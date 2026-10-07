# /// script
# requires-python = ">=3.12,<3.13"
# dependencies = ["ultralytics==8.4.174", "onnx==1.23.2", "onnxruntime==1.30.0"]
# ///
"""One-time model export. The running Telegram bot and inference are Rust only."""

import hashlib
import json
import os
from pathlib import Path
import shutil
import sys
import urllib.request

ROOT = Path(__file__).resolve().parent
MODELS = ROOT / "models"
REVISION = "a5382b48f32fab49f2881b18bc3125592c318c64"
CHECKPOINT_SHA256 = "386155cae09bee6af1ce99608fc42a32cafd40a25362b80037b4fa54f6999719"
URL = f"https://huggingface.co/Utbird/crack_detection/resolve/{REVISION}/crack.pt"


def main():
    MODELS.mkdir(parents=True, exist_ok=True)
    config_dir = MODELS / ".ultralytics"
    config_dir.mkdir(parents=True, exist_ok=True)
    os.environ["YOLO_CONFIG_DIR"] = str(config_dir)
    weights = MODELS / "crack.pt"
    if not weights.exists():
        partial = weights.with_suffix(".download")
        print("Downloading pinned crack-trained YOLO checkpoint...")
        urllib.request.urlretrieve(URL, partial)
        if hashlib.sha256(partial.read_bytes()).hexdigest() != CHECKPOINT_SHA256:
            partial.unlink()
            raise RuntimeError("Downloaded checkpoint failed SHA-256 verification")
        partial.replace(weights)
    if hashlib.sha256(weights.read_bytes()).hexdigest() != CHECKPOINT_SHA256:
        raise RuntimeError("Existing checkpoint failed SHA-256 verification")

    import onnx
    import onnxruntime
    from ultralytics import YOLO

    output = MODELS / "crack.onnx"
    if not output.exists():
        model = YOLO(str(weights))
        print("Checkpoint classes:", model.names)
        if len(model.names) != 1 or "crack" not in str(model.names[0]).lower():
            raise RuntimeError("Expected one crack class")
        exported = model.export(format="onnx", imgsz=640, batch=1, dynamic=False,
                                simplify=False, opset=17, nms=False, device="cpu")
        if Path(exported).resolve() != output.resolve():
            shutil.copy2(exported, output)
    onnx.checker.check_model(str(output))
    session = onnxruntime.InferenceSession(str(output), providers=["CPUExecutionProvider"])
    if session.get_inputs()[0].shape != [1, 3, 640, 640] or session.get_outputs()[0].shape[:2] != [1, 5]:
        raise RuntimeError("ONNX model does not match the Rust single-class YOLO decoder")
    runtime_dir = Path(onnxruntime.__file__).parent / "capi"
    if sys.platform == "darwin":
        libraries = list(runtime_dir.glob("libonnxruntime.*.dylib"))
        destination = MODELS / "libonnxruntime.dylib"
    elif sys.platform == "win32":
        libraries = list(runtime_dir.glob("onnxruntime.dll"))
        destination = MODELS / "onnxruntime.dll"
    else:
        libraries = list(runtime_dir.glob("libonnxruntime.so.*"))
        destination = MODELS / "libonnxruntime.so"
    if len(libraries) != 1:
        raise RuntimeError(f"Cannot identify the native ONNX runtime in {runtime_dir}")
    shutil.copy2(libraries[0], destination)
    for auxiliary in runtime_dir.glob("*providers_shared*"):
        shutil.copy2(auxiliary, MODELS / auxiliary.name)
    metadata = {"source": "Utbird/crack_detection", "revision": REVISION,
                "checkpoint_sha256": CHECKPOINT_SHA256,
                "onnx_sha256": hashlib.sha256(output.read_bytes()).hexdigest(),
                "runtime_version": onnxruntime.__version__, "input_size": 640}
    (MODELS / "source.json").write_text(json.dumps(metadata, indent=2) + "\n")
    print("Model and native CPU runtime ready in", MODELS)


if __name__ == "__main__":
    main()
