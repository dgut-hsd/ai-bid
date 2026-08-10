from __future__ import annotations

import argparse
import hashlib
import hmac
import json
import os
import re
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid
from collections import defaultdict
from datetime import datetime
from pathlib import Path


ROOT = Path(__file__).resolve().parent
DEFAULT_AGENTS = [
    "fact_check",
    "procedure",
    "rule_engine",
    "semantic_risk",
    "scoring",
    "demand",
    "contract",
]


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def verify_frozen_dataset(dataset_root: Path) -> dict | None:
    freeze_path = dataset_root / "data" / "freeze_manifest.json"
    if not freeze_path.exists():
        return None
    freeze = json.loads(freeze_path.read_text(encoding="utf-8"))
    mismatches = []
    for relative, expected in freeze.get("files", {}).items():
        target = dataset_root / relative
        actual = sha256(target) if target.exists() else "missing"
        if actual != expected:
            mismatches.append(
                {"file": relative, "expected": expected, "actual": actual}
            )
    if mismatches:
        raise RuntimeError(
            "盲测数据冻结校验失败，禁止运行: "
            + json.dumps(mismatches, ensure_ascii=False)
        )
    return freeze


def load_jsonl(path: Path) -> list[dict]:
    rows = []
    with path.open("r", encoding="utf-8-sig") as f:
        for line_number, line in enumerate(f, 1):
            if line.strip():
                try:
                    rows.append(json.loads(line))
                except json.JSONDecodeError as exc:
                    raise ValueError(f"{path}:{line_number}: {exc}") from exc
    return rows


def write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2), encoding="utf-8")


# ─── Rust 内部接口 HMAC 签名（与 backend-java InternalRequestSigner 对齐）───
# Rust server 对 /api/v1/* 要求 HMAC 签名；未配置 secret 时一律 503。
# benchmark 直连 Rust 引擎，必须生成相同的信封头。secret 从环境变量读取。

_INTERNAL_SECRET_ENVS = ("RUST_API_INTERNAL_SECRET", "AIBID_INTERNAL_API_SECRET")


def _internal_secret() -> str:
    for name in _INTERNAL_SECRET_ENVS:
        value = os.environ.get(name)
        if value:
            return value.strip()
    # fallback：从仓库根 .env 读取（Rust server 同样从该 .env 加载 secret）
    env_path = ROOT.parent / ".env"
    if env_path.exists():
        for line in env_path.read_text(encoding="utf-8").splitlines():
            line = line.strip()
            if line.startswith("RUST_API_INTERNAL_SECRET=") or line.startswith(
                "AIBID_INTERNAL_API_SECRET="
            ):
                value = line.split("=", 1)[1].strip()
                if value:
                    return value
    return ""


def _path_and_query(url: str) -> str:
    parsed = urllib.parse.urlsplit(url)
    path = parsed.path or "/"
    return path if not parsed.query else f"{path}?{parsed.query}"


def internal_auth_headers(method: str, url: str, body: bytes | None) -> dict[str, str]:
    """生成 Rust 内部接口签名头；secret 未配置时返回空 dict（server 侧等同 503）。"""
    secret = _internal_secret()
    if not secret:
        return {}
    method = method.upper()
    timestamp = str(int(time.time()))
    tenant_id = "1"
    user_id = "1"
    request_id = uuid.uuid4().hex
    body_sha256 = hashlib.sha256(body or b"").hexdigest()
    canonical = "\n".join(
        [
            "v1",
            method,
            _path_and_query(url),
            timestamp,
            tenant_id,
            user_id,
            request_id,
            body_sha256,
        ]
    )
    signature = hmac.new(
        secret.encode("utf-8"), canonical.encode("utf-8"), hashlib.sha256
    ).hexdigest()
    return {
        "X-Tenant-Id": tenant_id,
        "X-User-Id": user_id,
        "X-Request-Id": request_id,
        "X-Internal-Timestamp": timestamp,
        "X-Internal-Signature": f"v1={signature}",
    }


def request_json(
    method: str,
    url: str,
    payload: dict | None = None,
    *,
    timeout: int = 600,
) -> dict:
    data = None
    headers = {"Accept": "application/json"}
    if payload is not None:
        data = json.dumps(payload, ensure_ascii=False).encode("utf-8")
        headers["Content-Type"] = "application/json"
    headers.update(internal_auth_headers(method, url, data))
    request = urllib.request.Request(url, data=data, headers=headers, method=method)
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            body = response.read().decode("utf-8")
    except urllib.error.HTTPError as exc:
        body = exc.read().decode("utf-8", errors="replace")
        raise RuntimeError(f"HTTP {exc.code} {url}: {body}") from exc
    return json.loads(body) if body else {}


def upload_pdf(
    base_url: str,
    pdf_path: Path,
    timeout: int,
    desensitize_mode: str,
) -> dict:
    boundary = "----TenderBenchmark" + uuid.uuid4().hex
    safe_filename = pdf_path.name.encode("ascii", errors="ignore").decode() or "benchmark.pdf"
    mode_part = (
        f"--{boundary}\r\n"
        'Content-Disposition: form-data; name="desensitize_mode"\r\n\r\n'
        f"{desensitize_mode}\r\n"
    ).encode("ascii")
    prefix = (
        f"--{boundary}\r\n"
        f'Content-Disposition: form-data; name="file"; filename="{safe_filename}"\r\n'
        "Content-Type: application/pdf\r\n\r\n"
    ).encode("ascii")
    suffix = f"\r\n--{boundary}--\r\n".encode("ascii")
    body = mode_part + prefix + pdf_path.read_bytes() + suffix
    upload_headers = {
        "Content-Type": f"multipart/form-data; boundary={boundary}",
        "Accept": "application/json",
    }
    upload_headers.update(
        internal_auth_headers("POST", f"{base_url}/api/v1/documents", body)
    )
    request = urllib.request.Request(
        f"{base_url}/api/v1/documents",
        data=body,
        headers=upload_headers,
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            return json.loads(response.read().decode("utf-8"))
    except urllib.error.HTTPError as exc:
        error = exc.read().decode("utf-8", errors="replace")
        raise RuntimeError(f"上传失败 HTTP {exc.code}: {error}") from exc


def select_injection_chunks(
    base_url: str,
    engine_doc_id: str,
    gold_rows: list[dict],
    timeout: int,
) -> list[str]:
    response = request_json(
        "POST",
        f"{base_url}/api/v1/documents/{engine_doc_id}/search",
        {
            "queries": [row["source_quote"] for row in gold_rows],
            # 精确注入文本在部分超长 PDF 的向量排名仍可能落到前10之外；
            # 取足够大的候选集后再按目标页码做确定性过滤。
            "top_k": 1000,
        },
        timeout=timeout,
    )
    injection_pages = {int(row["page_number"]) for row in gold_rows}
    normalized_quotes = [
        re.sub(r"\s+", "", row["source_quote"]) for row in gold_rows
    ]
    chunk_ids = []
    for group in response.get("results", []):
        group_candidates = []
        for hit in group.get("hits", []):
            normalized_snippet = re.sub(r"\s+", "", str(hit.get("snippet", "")))
            quote_hit = any(
                quote in normalized_snippet or normalized_snippet in quote
                for quote in normalized_quotes
                if quote and normalized_snippet
            )
            page_number = int(hit.get("page_start", -1)) + 1
            # Rust Chunk.page_start 为 0-based；真值 page_number 为 1-based。
            # 合并 chunk 可能从前一页开始，而搜索 API 当前未返回 page_end。
            # 每个精确查询只取分数最高的目标页/前一页候选，避免扩大审查范围。
            page_hit = (
                page_number in injection_pages
                or any(page_number + 1 == target for target in injection_pages)
            )
            if quote_hit or page_hit:
                group_candidates.append(hit)
        if group_candidates:
            chunk_id = group_candidates[0].get("chunk_id")
            if chunk_id and chunk_id not in chunk_ids:
                chunk_ids.append(chunk_id)
    # 某些原 PDF 的页树/合并页导致 pypdf 页码与解析器 page_start 相差两页以上，
    # 且长 chunk 的搜索摘要可能截断了末尾注入文本。用中性的盲测页眉做定位兜底，
    # 不包含风险类别或答案，不改变模型输入和评分。
    if not chunk_ids:
        document_id = str(gold_rows[0].get("document_id", ""))
        fallback = request_json(
            "POST",
            f"{base_url}/api/v1/documents/{engine_doc_id}/search",
            {
                "queries": [f"采购文件补充条款 文件编号 {document_id}"],
                "top_k": 1000,
            },
            timeout=timeout,
        )
        normalized_id = re.sub(r"\s+", "", document_id)
        for group in fallback.get("results", []):
            for hit in group.get("hits", []):
                snippet = re.sub(r"\s+", "", str(hit.get("snippet", "")))
                if (
                    normalized_id
                    and normalized_id in snippet
                    and "补充条款" in snippet
                ):
                    chunk_id = hit.get("chunk_id")
                    if chunk_id and chunk_id not in chunk_ids:
                        chunk_ids.append(chunk_id)
                    break
    if not chunk_ids:
        raise RuntimeError(
            f"未能从注入页定位条款块，目标页={sorted(injection_pages)}；"
            "请检查PDF解析或改用 --scope full"
        )
    return chunk_ids


def wait_for_result(
    base_url: str,
    engine_doc_id: str,
    *,
    poll_seconds: int,
    review_timeout: int,
) -> dict:
    deadline = time.monotonic() + review_timeout
    while time.monotonic() < deadline:
        result = request_json(
            "GET",
            f"{base_url}/api/v1/review/{engine_doc_id}/result",
            timeout=60,
        )
        status = result.get("status")
        if status == "completed":
            return result
        if status == "failed":
            raise RuntimeError(result.get("error") or "审核任务失败")
        time.sleep(poll_seconds)
    raise TimeoutError(f"审核超时（>{review_timeout}秒）")


def normalize_finding(document_id: str, finding: dict) -> dict:
    normalized = dict(finding)
    normalized["document_id"] = document_id
    if "risk_id" not in normalized and "issue_no" in normalized:
        normalized["risk_id"] = normalized["issue_no"]
    if "risk_type" not in normalized and "category" in normalized:
        normalized["risk_type"] = normalized["category"]
    if "source_quote" not in normalized and "context" in normalized:
        normalized["source_quote"] = normalized["context"]
    return normalized


def main() -> int:
    parser = argparse.ArgumentParser(description="无人值守运行标书审核基准并计算上线门槛")
    parser.add_argument("--base-url", default="http://127.0.0.1:3001")
    parser.add_argument("--split", default="test", choices=["train", "dev", "test"])
    parser.add_argument(
        "--scope",
        default="injected",
        choices=["injected", "full"],
        help="injected仅审查注入页；full审查整份文件",
    )
    parser.add_argument("--run-id", default=None)
    parser.add_argument("--agents", nargs="*", default=DEFAULT_AGENTS)
    parser.add_argument("--poll-seconds", type=int, default=5)
    parser.add_argument("--upload-timeout", type=int, default=900)
    parser.add_argument("--review-timeout", type=int, default=1800)
    parser.add_argument("--limit", type=int, default=None, help="仅跑前N份，用于探针")
    parser.add_argument(
        "--documents",
        default="",
        help="逗号分隔的 document_id；用于精确选择定向探针文档",
    )
    parser.add_argument("--no-resume", action="store_true")
    parser.add_argument(
        "--desensitize-mode",
        default="low",
        choices=["off", "low"],
        help="上传到审核引擎时的文件脱敏等级；正式验收默认 low",
    )
    parser.add_argument(
        "--dataset-root",
        default=str(ROOT),
        help="数据集根目录；默认 benchmark，blind-v2 可指向 benchmark/blind-v2",
    )
    args = parser.parse_args()
    dataset_root = Path(args.dataset_root).resolve()
    gold_path = dataset_root / "data" / "annotations.jsonl"
    if not gold_path.exists():
        raise FileNotFoundError(f"缺少真值文件: {gold_path}")
    freeze = verify_frozen_dataset(dataset_root)

    base_url = args.base_url.rstrip("/")
    health = request_json("GET", f"{base_url}/health", timeout=15)
    if health.get("status") != "ok":
        raise RuntimeError(f"审核服务健康检查失败: {health}")

    gold_rows = [
        row for row in load_jsonl(gold_path)
        if row.get("split") == args.split
    ]
    gold_by_doc: dict[str, list[dict]] = defaultdict(list)
    for row in gold_rows:
        gold_by_doc[row["document_id"]].append(row)
    document_ids = sorted(gold_by_doc)
    selected_documents = {
        value.strip() for value in args.documents.split(",") if value.strip()
    }
    if selected_documents:
        unknown = selected_documents.difference(document_ids)
        if unknown:
            raise RuntimeError(f"split={args.split} 不包含文档: {sorted(unknown)}")
        document_ids = [
            document_id
            for document_id in document_ids
            if document_id in selected_documents
        ]
    if args.limit is not None:
        document_ids = document_ids[: args.limit]
    if not document_ids:
        raise RuntimeError(f"split={args.split} 没有可运行文档")

    run_id = args.run_id or datetime.now().strftime(
        f"%Y%m%d-%H%M%S-{args.split}-{args.scope}"
    )
    run_dir = dataset_root / "results" / run_id
    doc_dir = run_dir / "documents"
    doc_dir.mkdir(parents=True, exist_ok=True)

    run_manifest = {
        "run_id": run_id,
        "benchmark": (
            freeze.get("benchmark_version")
            if freeze
            else (gold_rows[0].get("benchmark_version") or "silver-v1.0")
        ),
        "dataset_root": str(dataset_root),
        "freeze_sha256": sha256(dataset_root / "data" / "freeze_manifest.json")
        if freeze
        else None,
        "split": args.split,
        "scope": args.scope,
        "base_url": base_url,
        "agents": args.agents,
        "documents": document_ids,
        "started_at": datetime.now().isoformat(timespec="seconds"),
    }
    write_json(run_dir / "run_manifest.json", run_manifest)

    predictions: list[dict] = []
    failures: list[dict] = []
    durations: dict[str, float] = {}
    usages: dict[str, dict] = {}
    completed = 0

    for position, document_id in enumerate(document_ids, 1):
        output_path = doc_dir / f"{document_id}.json"
        if output_path.exists() and not args.no_resume:
            cached = json.loads(output_path.read_text(encoding="utf-8"))
            if cached.get("status") == "completed":
                findings = cached.get("predictions", [])
                predictions.extend(findings)
                durations[document_id] = float(cached.get("duration_seconds", 0))
                if cached.get("usage"):
                    usages[document_id] = cached.get("usage")
                completed += 1
                print(f"[{position}/{len(document_ids)}] {document_id} 复用已完成结果")
                continue

        gold_for_doc = gold_by_doc[document_id]
        pdf_relative = gold_for_doc[0]["mutated_file"]
        pdf_path = dataset_root / pdf_relative
        started = time.monotonic()
        print(f"[{position}/{len(document_ids)}] {document_id} 上传并解析 {pdf_path.name}", flush=True)

        try:
            upload = upload_pdf(
                base_url,
                pdf_path,
                args.upload_timeout,
                args.desensitize_mode,
            )
            engine_doc_id = upload["document_id"]
            chunk_ids: list[str] = []
            if args.scope == "injected":
                chunk_ids = select_injection_chunks(
                    base_url, engine_doc_id, gold_for_doc, args.upload_timeout
                )
                print(
                    f"  定位注入页：{len(chunk_ids)}个条款块；"
                    f"全文共{upload.get('total_chunks')}个条款块",
                    flush=True,
                )

            review_payload = {
                "chunk_ids": chunk_ids,
                "enabled_agents": args.agents,
            }
            if chunk_ids:
                review_payload["max_clauses"] = len(chunk_ids)
            accepted = request_json(
                "POST",
                f"{base_url}/api/v1/documents/{engine_doc_id}/review",
                review_payload,
                timeout=60,
            )
            if accepted.get("status") not in {"accepted", "conflict"}:
                raise RuntimeError(f"审核请求未被接受: {accepted}")

            result = wait_for_result(
                base_url,
                engine_doc_id,
                poll_seconds=args.poll_seconds,
                review_timeout=args.review_timeout,
            )
            raw_findings = (result.get("result") or {}).get("findings", [])
            doc_predictions = [
                normalize_finding(document_id, finding)
                for finding in raw_findings
            ]
            duration = time.monotonic() - started
            predictions.extend(doc_predictions)
            durations[document_id] = duration
            completed += 1
            usage = result.get("usage") or {}
            if usage:
                usages[document_id] = usage
            write_json(output_path, {
                "status": "completed",
                "document_id": document_id,
                "engine_document_id": engine_doc_id,
                "upload": upload,
                "selected_chunk_ids": chunk_ids,
                "duration_seconds": round(duration, 2),
                "usage": usage,
                "predictions": doc_predictions,
            })
            print(
                f"  完成：{len(doc_predictions)}条发现，耗时{duration / 60:.1f}分钟",
                flush=True,
            )
        except Exception as exc:
            duration = time.monotonic() - started
            failure = {
                "document_id": document_id,
                "error": str(exc),
                "duration_seconds": round(duration, 2),
            }
            failures.append(failure)
            write_json(output_path, {"status": "failed", **failure})
            print(f"  失败：{exc}", file=sys.stderr, flush=True)

        progress = {
            "total_documents": len(document_ids),
            "completed": completed,
            "failed": len(failures),
            "predictions": len(predictions),
            "updated_at": datetime.now().isoformat(timespec="seconds"),
        }
        write_json(run_dir / "progress.json", progress)

    predictions_path = run_dir / "predictions.jsonl"
    with predictions_path.open("w", encoding="utf-8") as f:
        for prediction in predictions:
            f.write(json.dumps(prediction, ensure_ascii=False) + "\n")

    metrics_path = run_dir / "metrics.json"
    # Windows 下子进程 stdout 默认用控制台 code page（如 GBK），
    # 而 evaluate.py 输出 ensure_ascii=False 的中文 JSON；
    # 强制子进程以 UTF-8 输出，避免 UnicodeDecodeError 导致 stdout=None。
    evaluate_env = {**os.environ, "PYTHONIOENCODING": "utf-8"}
    evaluate = subprocess.run(
        [
            sys.executable,
            str(ROOT / "evaluate.py"),
            "--gold",
            str(gold_path),
            "--pred",
            str(predictions_path),
            "--include-splits",
            args.split,
            "--include-documents",
            ",".join(document_ids),
            "--output",
            str(metrics_path),
        ],
        env=evaluate_env,
        check=True,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    metrics = json.loads(evaluate.stdout)
    all_completed = completed == len(document_ids) and not failures
    gate_passed = bool(metrics["release_gate"]["passed"] and all_completed)

    summary = {
        **run_manifest,
        "finished_at": datetime.now().isoformat(timespec="seconds"),
        "completed_documents": completed,
        "failed_documents": len(failures),
        "prediction_count": len(predictions),
        "duration_seconds": round(sum(durations.values()), 2),
        "metrics": {
            "precision": metrics["overall"]["precision"],
            "recall": metrics["overall"]["recall"],
            "f1": metrics["overall"]["f1"],
            "critical_recall": metrics["critical"]["recall"],
            "critical_detection_recall": metrics["critical"].get("detection_recall", 0),
            "critical_severity_recall": metrics["critical"].get("severity_recall", 0),
            "severity_agreement_on_matches": metrics.get("severity_agreement_on_matches", 0),
        },
        "token_usage": {
            "documents_with_usage": len(usages),
            "llm_calls": sum(u.get("llm_calls", 0) for u in usages.values()),
            "tokens_input": sum(u.get("tokens_input", 0) for u in usages.values()),
            "tokens_output": sum(u.get("tokens_output", 0) for u in usages.values()),
            "cost_cny": round(sum(u.get("cost_cny", 0) for u in usages.values()), 2),
        },
        "release_gate_passed": gate_passed,
        "failures": failures,
    }
    write_json(run_dir / "summary.json", summary)
    write_json(run_dir / "token_usage.json", summary["token_usage"])
    (run_dir / "summary.md").write_text(
        "\n".join([
            f"# 标书审核验收结果：{'通过' if gate_passed else '未通过'}",
            "",
            f"- 运行编号：`{run_id}`",
            f"- 数据集：`{args.split}`，范围：`{args.scope}`",
            f"- 完成文档：{completed}/{len(document_ids)}",
            f"- LLM 调用：{summary['token_usage']['llm_calls']} 次 | tokens "
            f"{summary['token_usage']['tokens_input']:,} in / {summary['token_usage']['tokens_output']:,} out "
            f"| 成本 ¥{summary['token_usage']['cost_cny']:.2f}",
            f"- Precision：{metrics['overall']['precision']:.2%}",
            f"- Recall：{metrics['overall']['recall']:.2%}",
            f"- F1：{metrics['overall']['f1']:.2%}（门槛 80%）",
            f"- Critical 检出率：{metrics['critical'].get('detection_recall', 0):.2%}",
            f"- Critical 标记召回率：{metrics['critical']['recall']:.2%}（门槛 95%）",
            f"- Critical 严重度判定正确率：{metrics['critical'].get('severity_recall', 0):.2%}",
            f"- 已命中问题的严重度一致率：{metrics.get('severity_agreement_on_matches', 0):.2%}",
            f"- 最终门禁：{'PASS' if gate_passed else 'FAIL'}",
            "",
            "> injected 模式仅证明注入问题识别能力；生产上线仍需 full 模式及真实原文双人标注。",
        ]),
        encoding="utf-8",
    )

    print(json.dumps(summary, ensure_ascii=False, indent=2))
    print(f"验收报告：{run_dir / 'summary.md'}")
    return 0 if gate_passed else 2


if __name__ == "__main__":
    raise SystemExit(main())
