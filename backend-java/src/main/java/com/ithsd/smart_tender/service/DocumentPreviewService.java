package com.ithsd.smart_tender.service;

import org.springframework.beans.factory.annotation.Value;
import org.springframework.core.io.ByteArrayResource;
import org.springframework.http.HttpHeaders;
import org.springframework.http.MediaType;
import org.springframework.http.ResponseEntity;
import org.springframework.stereotype.Service;

import java.io.IOException;
import java.net.URLEncoder;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.nio.file.StandardOpenOption;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.Comparator;
import java.util.List;
import java.util.concurrent.Semaphore;
import java.util.concurrent.TimeUnit;

@Service
public class DocumentPreviewService {

    /** 最大并发转换数：LibreOffice headless 单实例较重，限制并发避免内存/CPU 尖峰。 */
    private static final int MAX_CONCURRENT_CONVERSIONS = 2;

    /** 等待转换名额的最长时间（秒）。 */
    private static final long CONVERT_WAIT_SECONDS = 30;

    /** 单次 soffice 转换超时（秒），超时后强制终止进程。 */
    private static final long CONVERT_TIMEOUT_SECONDS = 60;

    private static final Semaphore CONVERSION_LIMIT = new Semaphore(MAX_CONCURRENT_CONVERSIONS);

    @Value("${preview.cache.path:}")
    private String previewCachePath;

    public ResponseEntity<ByteArrayResource> convertDocxToPdf(Path sourcePath, String downloadFileName) throws IOException {
        Path pdfPath = ensurePdfPreviewFile(sourcePath);
        byte[] pdfBytes = Files.readAllBytes(pdfPath);
        ByteArrayResource pdfResource = new ByteArrayResource(pdfBytes);
        String finalName = (downloadFileName == null || downloadFileName.isBlank())
                ? "document.pdf"
                : downloadFileName + ".pdf";
        String encodedName = URLEncoder.encode(finalName, StandardCharsets.UTF_8).replace("+", "%20");

        return ResponseEntity.ok()
                .header(HttpHeaders.CONTENT_DISPOSITION, "inline; filename*=UTF-8''" + encodedName)
                .contentType(MediaType.APPLICATION_PDF)
                .body(pdfResource);
    }

    public Path ensurePdfPreviewFile(Path sourcePath) throws IOException {
        String name = sourcePath.getFileName().toString().toLowerCase();
        if (name.endsWith(".pdf")) {
            return sourcePath;
        }
        if (!name.endsWith(".doc") && !name.endsWith(".docx")) {
            throw new IOException("仅支持Word文档转PDF: " + sourcePath);
        }
        Path target = buildPreviewPdfPath(sourcePath);
        if (Files.exists(target) && Files.getLastModifiedTime(target).toMillis() >= Files.getLastModifiedTime(sourcePath).toMillis()) {
            return target;
        }
        byte[] pdfBytes = convertToPdfBytes(sourcePath);
        Files.write(target, pdfBytes, StandardOpenOption.CREATE, StandardOpenOption.TRUNCATE_EXISTING, StandardOpenOption.WRITE);
        return target;
    }

    /**
     * 使用本地 LibreOffice（soffice headless）将 .doc/.docx 转为 PDF 字节。
     *
     * <p>相比原先调用 JODConverter REST 服务（需额外部署 doc-converter 容器），
     * 这里直接调用容器内已安装的 LibreOffice，避免额外服务依赖。</p>
     *
     * <p>关键防护：每次转换使用独立的 UserInstallation profile，规避并发时的
     * profile 锁冲突；用信号量限制并发数；对进程设置超时并强制终止。</p>
     */
    public byte[] convertToPdfBytes(Path sourcePath) throws IOException {
        String name = sourcePath.getFileName().toString().toLowerCase();
        if (!name.endsWith(".doc") && !name.endsWith(".docx")) {
            throw new IOException("仅支持Word文档转PDF: " + sourcePath);
        }

        Path tempDir = Files.createTempDirectory("lo-convert-");
        try {
            boolean acquired = false;
            try {
                acquired = CONVERSION_LIMIT.tryAcquire(CONVERT_WAIT_SECONDS, TimeUnit.SECONDS);
            } catch (InterruptedException e) {
                Thread.currentThread().interrupt();
                throw new IOException("等待文档转换资源时被中断", e);
            }
            if (!acquired) {
                throw new IOException("文档转换服务繁忙，请稍后重试");
            }
            try {
                return runSofficeConversion(sourcePath, tempDir);
            } finally {
                CONVERSION_LIMIT.release();
            }
        } finally {
            deleteRecursively(tempDir);
        }
    }

    private byte[] runSofficeConversion(Path sourcePath, Path tempDir) throws IOException {
        Path outDir = Files.createDirectories(tempDir.resolve("out"));
        Path profileDir = Files.createDirectories(tempDir.resolve("profile"));
        Path logFile = tempDir.resolve("soffice.log");

        List<String> command = List.of(
                resolveSoffice(),
                "--headless",
                "--nologo",
                "--nofirststartwizard",
                "--nodefault",
                "-env:UserInstallation=file://" + profileDir.toAbsolutePath(),
                "--convert-to", "pdf",
                "--outdir", outDir.toAbsolutePath().toString(),
                sourcePath.toAbsolutePath().toString()
        );

        ProcessBuilder builder = new ProcessBuilder(command);
        builder.redirectErrorStream(true);
        builder.redirectOutput(logFile.toFile());

        Process process;
        try {
            process = builder.start();
        } catch (IOException e) {
            throw new IOException("无法启动 LibreOffice (soffice)，请确认已安装: " + e.getMessage(), e);
        }

        boolean finished;
        try {
            finished = process.waitFor(CONVERT_TIMEOUT_SECONDS, TimeUnit.SECONDS);
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            killProcessTree(process);
            throw new IOException("等待 LibreOffice 转换时被中断", e);
        }

        if (!finished) {
            killProcessTree(process);
            throw new IOException("文档转换超时（超过 " + CONVERT_TIMEOUT_SECONDS + " 秒），已终止转换进程");
        }

        if (process.exitValue() != 0) {
            String log = readLog(logFile);
            throw new IOException("文档转换失败，退出码 " + process.exitValue() + (log.isBlank() ? "" : ": " + log));
        }

        Path pdf = outDir.resolve(stemOf(sourcePath) + ".pdf");
        if (!Files.exists(pdf)) {
            throw new IOException("LibreOffice 未生成 PDF 文件: " + pdf);
        }
        return Files.readAllBytes(pdf);
    }

    private String resolveSoffice() throws IOException {
        String env = System.getenv("LIBREOFFICE_PATH");
        if (env != null && !env.isBlank()) {
            Path candidate = Paths.get(env);
            if (Files.isExecutable(candidate)) {
                return candidate.toAbsolutePath().toString();
            }
        }
        // 依赖 PATH（Linux 容器内通常为 /usr/bin/soffice）
        return "soffice";
    }

    private void killProcessTree(Process process) {
        try {
            process.descendants().forEach(handle -> {
                try {
                    handle.destroyForcibly();
                } catch (Exception ignored) {
                }
            });
        } catch (Exception ignored) {
        }
        process.destroyForcibly();
        try {
            process.waitFor(2, TimeUnit.SECONDS);
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
        }
    }

    private String readLog(Path logFile) {
        try {
            if (Files.exists(logFile)) {
                String content = Files.readString(logFile, StandardCharsets.UTF_8).trim();
                if (content.length() > 2000) {
                    content = content.substring(content.length() - 2000);
                }
                return content;
            }
        } catch (IOException ignored) {
        }
        return "";
    }

    private String stemOf(Path path) {
        String name = path.getFileName().toString();
        int dot = name.lastIndexOf('.');
        return dot > 0 ? name.substring(0, dot) : name;
    }

    private void deleteRecursively(Path root) {
        if (root == null || !Files.exists(root)) {
            return;
        }
        try (var stream = Files.walk(root)) {
            stream.sorted(Comparator.reverseOrder()).forEach(path -> {
                try {
                    Files.deleteIfExists(path);
                } catch (IOException ignored) {
                }
            });
        } catch (IOException ignored) {
        }
    }

    private Path buildPreviewPdfPath(Path sourcePath) throws IOException {
        Path cacheRoot;
        if (previewCachePath == null || previewCachePath.isBlank()) {
            cacheRoot = sourcePath.getParent().resolve(".preview-cache");
        } else {
            cacheRoot = Paths.get(previewCachePath);
        }
        Files.createDirectories(cacheRoot);
        String cacheKey = buildCacheKey(sourcePath);
        return cacheRoot.resolve(cacheKey + ".preview.pdf");
    }

    private String buildCacheKey(Path sourcePath) throws IOException {
        String seed = sourcePath.toAbsolutePath().normalize() + "|" + Files.getLastModifiedTime(sourcePath).toMillis();
        try {
            MessageDigest digest = MessageDigest.getInstance("SHA-256");
            byte[] bytes = digest.digest(seed.getBytes(StandardCharsets.UTF_8));
            StringBuilder builder = new StringBuilder();
            for (byte value : bytes) {
                builder.append(String.format("%02x", value));
            }
            return builder.substring(0, 24);
        } catch (NoSuchAlgorithmException ex) {
            throw new IOException("预览缓存键生成失败", ex);
        }
    }
}
