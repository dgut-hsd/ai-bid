//! DOCX → PDF 转换服务
//!
//! 通过调用 LibreOffice headless 模式将 .docx 文件转换为 PDF，
//! 转换后的 PDF 可直接输入现有的 PDF 提取管线。
//!
//! ## 依赖
//!
//! 需要系统已安装 LibreOffice（Windows / Linux / macOS 均支持）。
//! Linux 下以专用非 root 系统用户 `soffice` 运行（见 Dockerfile），
//! 降低处理不可信文档时的权限。
//! 默认搜索路径：
//!   - Windows: `C:\Program Files\LibreOffice\program\soffice.exe`
//!   - Linux/macOS: `soffice` (PATH 中)

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// 搜索 LibreOffice 可执行文件路径。
///
/// 按以下顺序查找：
/// 1. 环境变量 `LIBREOFFICE_PATH`
/// 2. Windows 默认安装路径
/// 3. PATH 中的 `soffice`
fn find_soffice() -> Result<PathBuf> {
    // 1. 环境变量
    if let Ok(path) = std::env::var("LIBREOFFICE_PATH") {
        let p = Path::new(&path);
        if p.exists() {
            return Ok(p.to_path_buf());
        }
    }

    // 2. Windows 默认路径
    #[cfg(windows)]
    {
        let candidates = [
            r"C:\Program Files\LibreOffice\program\soffice.exe",
            r"C:\Program Files (x86)\LibreOffice\program\soffice.exe",
        ];
        for candidate in &candidates {
            let p = Path::new(candidate);
            if p.exists() {
                return Ok(p.to_path_buf());
            }
        }
    }

    // 3. PATH 中的 soffice (Linux/macOS)
    let which_cmd = if cfg!(windows) { "where" } else { "which" };
    if let Ok(output) = Command::new(which_cmd).arg("soffice").output()
        && output.status.success()
    {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let p = Path::new(&path);
        if p.exists() {
            return Ok(p.to_path_buf());
        }
    }

    anyhow::bail!(
        "找不到 LibreOffice (soffice)。请安装 LibreOffice 或设置 LIBREOFFICE_PATH 环境变量。\n\
         下载: https://www.libreoffice.org/download/"
    )
}

/// 将 DOCX/DOC 文件转换为 PDF。
///
/// # 参数
///
/// * `input_path` - 输入的 .docx / .doc 文件路径
/// * `output_dir` - 输出目录（生成的 PDF 放在此处，文件名与输入同 stem）
///
/// # 安全 / 并发说明
///
/// 在 Linux 上，LibreOffice 以专用的非 root 系统用户（`soffice`）运行，
/// 且每次转换使用独立的 `UserInstallation` profile 与独立的临时输出目录，
/// 既避免并发时的 profile 锁冲突，也降低处理不可信文档时的权限。
///
/// # 返回
///
/// 成功时返回生成的 PDF 文件路径。
///
/// # 示例
///
/// ```ignore
/// let pdf_path = convert_docx_to_pdf("tests/投标文件.docx", "tests/")?;
/// // → "tests/投标文件.pdf"
/// ```
pub fn convert_docx_to_pdf(input_path: &str, output_dir: &str) -> Result<PathBuf> {
    let input = Path::new(input_path);

    // 验证输入文件存在
    anyhow::ensure!(input.exists(), "输入文件不存在: {}", input.display());

    // 检查扩展名
    let ext = input
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    anyhow::ensure!(
        ext == "docx" || ext == "doc",
        "不支持的文件格式: .{}，仅支持 .docx / .doc",
        ext
    );

    let soffice = find_soffice()?;
    let input_abs = input.canonicalize()?;
    let output_dir_abs = Path::new(output_dir)
        .canonicalize()
        .with_context(|| format!("输出目录不存在或无法访问: {}", output_dir))?;
    let stem = input.file_stem().unwrap().to_string_lossy().to_string();

    println!(
        "  [转换] {} → PDF (LibreOffice)",
        input.file_name().unwrap().to_string_lossy()
    );

    // 独立临时工作目录：out=输出、profile=隔离的用户配置（规避并发 profile 锁）
    let work_root = std::env::temp_dir().join(format!("lo-convert-{}", uuid::Uuid::new_v4()));
    let work_out = work_root.join("out");
    let work_profile = work_root.join("profile");
    std::fs::create_dir_all(&work_out)?;
    std::fs::create_dir_all(&work_profile)?;
    let _guard = WorkDirGuard(work_root.clone());

    // 非 root 运行需要让 soffice 用户可写这些临时目录
    #[cfg(target_os = "linux")]
    {
        make_world_writable(&work_root)?;
        make_world_writable(&work_out)?;
        make_world_writable(&work_profile)?;
    }

    // Linux：以非 root 专用用户运行，隔离 profile，输出到临时目录
    #[cfg(target_os = "linux")]
    let output = Command::new("runuser")
        .arg("-u")
        .arg("soffice")
        .arg("--")
        .arg(&soffice)
        .arg("--headless")
        .arg("--nologo")
        .arg("--nofirststartwizard")
        .arg("--nodefault")
        .arg(format!("-env:UserInstallation=file://{}", work_profile.display()))
        .arg("--convert-to")
        .arg("pdf")
        .arg("--outdir")
        .arg(&work_out)
        .arg(&input_abs)
        .output()
        .with_context(|| {
            format!(
                "无法以非 root 用户执行 LibreOffice(soffice)。请确认已安装 LibreOffice 且已创建 soffice 用户: {}",
                soffice.display()
            )
        })?;

    // Windows / macOS：直接以当前用户运行，输出到 output_dir（保留原行为）
    #[cfg(not(target_os = "linux"))]
    let output = Command::new(&soffice)
        .arg("--headless")
        .arg("--convert-to")
        .arg("pdf")
        .arg("--outdir")
        .arg(&output_dir_abs)
        .arg(&input_abs)
        .output()
        .with_context(|| {
            format!(
                "无法执行 LibreOffice: {}。请确认已安装。",
                soffice.display()
            )
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        anyhow::bail!(
            "LibreOffice 转换失败:\nstdout: {}\nstderr: {}",
            stdout.trim(),
            stderr.trim()
        );
    }

    let pdf_path = output_dir_abs.join(format!("{}.pdf", stem));

    // Linux 下产物先写在临时目录，拷回调用方期望的 output_dir
    #[cfg(target_os = "linux")]
    {
        let produced = work_out.join(format!("{}.pdf", stem));
        anyhow::ensure!(
            produced.exists(),
            "LibreOffice 未生成 PDF 文件: {}",
            produced.display()
        );
        std::fs::copy(&produced, &pdf_path)
            .with_context(|| format!("无法将转换结果写入输出目录: {}", pdf_path.display()))?;
    }

    #[cfg(not(target_os = "linux"))]
    {
        anyhow::ensure!(
            pdf_path.exists(),
            "LibreOffice 未生成 PDF 文件: {}",
            pdf_path.display()
        );
    }

    // 打印转换日志
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.is_empty() {
        println!("  {}", stdout.trim());
    }

    Ok(pdf_path)
}

/// 将路径权限设为 rwxrwxrwx（供非 root 的 soffice 用户写入临时工作目录）。
#[cfg(target_os = "linux")]
fn make_world_writable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o777))?;
    Ok(())
}

/// 临时工作目录守护：Drop 时递归删除（防止转换失败造成 /tmp 残留）。
struct WorkDirGuard(PathBuf);

impl Drop for WorkDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_soffice() {
        let result = find_soffice();
        assert!(result.is_ok(), "LibreOffice 未安装: {:?}", result.err());
    }
}
