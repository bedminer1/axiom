"""Run the custom RoboMaster armor detector against the newest RTMP frame.

This mirrors 27_aimbot_software's YOLOv5 preprocessing and 22-value output
decoder, then publishes normalized plate quadrilaterals for the web overlay.
"""

from __future__ import annotations

import argparse
import json
import math
import os
import threading
import time
from collections import deque
from pathlib import Path

import cv2
import numpy as np

COLORS = ("blue", "red", "unlit", "purple")
ROBOT_TYPES = ("sentry", "1", "2", "3", "4", "5", "outpost", "base", "not armor")


# Continuously decodes the RTMP feed while retaining only its newest frame.
class LatestFrame:
    """Background capture that prevents inference from accumulating latency."""

    def __init__(self, source: str) -> None:
        self.source = source
        self.frame = None
        self.updated_at = 0.0
        self.lock = threading.Lock()
        self.running = True
        self.thread = threading.Thread(target=self._capture, daemon=True)
        self.thread.start()

    # Reconnects automatically and atomically replaces the latest frame.
    def _capture(self) -> None:
        while self.running:
            capture = cv2.VideoCapture(self.source, cv2.CAP_FFMPEG)
            capture.set(cv2.CAP_PROP_BUFFERSIZE, 1)
            while self.running:
                ok, frame = capture.read()
                if not ok:
                    break
                with self.lock:
                    self.frame = frame
                    self.updated_at = time.monotonic()
            capture.release()
            time.sleep(0.5)

    # Returns a copy so inference never blocks the decoding thread.
    def latest(self):
        with self.lock:
            if self.frame is None or time.monotonic() - self.updated_at > 2:
                return None
            return self.frame.copy()

    # Stops the capture thread during an orderly shutdown.
    def close(self) -> None:
        self.running = False
        self.thread.join(timeout=2)


# Implements the trained model's exact preprocessing and output contract.
class ArmorDetector:
    """OpenCV-DNN runner for the custom RoboMaster armor YOLOv5 model."""

    def __init__(self, model_path: Path, confidence: float, nms: float) -> None:
        self.net = cv2.dnn.readNetFromONNX(str(model_path))
        self.confidence = confidence
        self.nms = nms

    # Resizes into the upper-left of a gray 640-square, matching the C++ backend.
    @staticmethod
    def _preprocess(frame: np.ndarray) -> tuple[np.ndarray, float]:
        height, width = frame.shape[:2]
        scale = min(640 / width, 640 / height)
        resized = cv2.resize(frame, (round(width * scale), round(height * scale)))
        canvas = np.full((640, 640, 3), 114, dtype=np.uint8)
        canvas[: resized.shape[0], : resized.shape[1]] = resized
        return cv2.dnn.blobFromImage(canvas, 1 / 255.0, (640, 640), swapRB=True), scale

    # Applies a numerically stable sigmoid to the raw objectness logit.
    @staticmethod
    def _sigmoid(value: float) -> float:
        if value >= 0:
            return 1.0 / (1.0 + math.exp(-value))
        exponential = math.exp(value)
        return exponential / (1.0 + exponential)

    # Rejects invalid sticker/type combinations in the same way as the C++ app.
    @staticmethod
    def _valid_identity(number_id: int) -> bool:
        if number_id == 8:
            return False
        return number_id not in ((0, 2, 6) if number_id == 1 else (1, 7))

    # Runs one frame and returns normalized quadrilaterals for the web overlay.
    def detect(self, frame: np.ndarray) -> list[dict]:
        height, width = frame.shape[:2]
        blob, scale = self._preprocess(frame)
        self.net.setInput(blob)
        output = self.net.forward().reshape(-1, 22)
        candidates: list[dict] = []
        nms_boxes: list[list[int]] = []
        scores: list[float] = []
        for row in output:
            confidence = self._sigmoid(float(row[8]))
            if confidence < self.confidence:
                continue
            color_id = int(np.argmax(row[9:13]))
            number_id = int(np.argmax(row[13:22]))
            if not self._valid_identity(number_id):
                continue
            # Network order is TL, BL, BR, TR; browser order is clockwise.
            points = np.array(
                [[row[0], row[1]], [row[6], row[7]], [row[4], row[5]], [row[2], row[3]]],
                dtype=np.float32,
            ) / scale
            points[:, 0] = np.clip(points[:, 0], 0, width - 1)
            points[:, 1] = np.clip(points[:, 1], 0, height - 1)
            x1, y1 = points.min(axis=0)
            x2, y2 = points.max(axis=0)
            if x2 - x1 < 2 or y2 - y1 < 2:
                continue
            nms_boxes.append([round(x1), round(y1), round(x2 - x1), round(y2 - y1)])
            scores.append(confidence)
            color, robot_type = COLORS[color_id], ROBOT_TYPES[number_id]
            candidates.append({
                "label": f"{color} · {robot_type}",
                "color": color,
                "robot_type": robot_type,
                "confidence": round(confidence, 3),
                "x": round(float(x1 / width), 5), "y": round(float(y1 / height), 5),
                "width": round(float((x2 - x1) / width), 5),
                "height": round(float((y2 - y1) / height), 5),
                "points": [{"x": round(float(x / width), 5), "y": round(float(y / height), 5)} for x, y in points],
            })
        indices = cv2.dnn.NMSBoxes(nms_boxes, scores, self.confidence, self.nms)
        return [candidates[int(index)] for index in np.asarray(indices).reshape(-1)]


# Replaces the JSON file atomically so browsers never observe partial content.
def write_payload(output: Path, payload: dict) -> None:
    temporary = output.with_suffix(".tmp")
    temporary.write_text(json.dumps(payload, separators=(",", ":")))
    os.replace(temporary, output)


# Samples only the latest frame at detector rate, independently of display FPS.
def run(source: str, output: Path, model: Path, fps: float, confidence: float, nms: float) -> None:
    output.parent.mkdir(parents=True, exist_ok=True)
    detector, capture = ArmorDetector(model, confidence, nms), LatestFrame(source)
    history: deque[dict] = deque(maxlen=max(15, round(fps * 8)))
    interval = 1.0 / fps
    try:
        while True:
            started = time.monotonic()
            frame = capture.latest()
            if frame is not None:
                inference_started = time.perf_counter()
                boxes = detector.detect(frame)
                history.append({
                    "captured_at_ms": round(time.time() * 1000),
                    "inference_ms": round((time.perf_counter() - inference_started) * 1000, 1),
                    "boxes": boxes,
                })
                write_payload(output, {
                    "status": "live", "detector": "RoboMaster armor YOLOv5",
                    "sample_fps": fps, "frames": list(history),
                })
            time.sleep(max(0, interval - (time.monotonic() - started)))
    finally:
        capture.close()


# Parses launcher settings and starts the long-running detector.
def main() -> None:
    default_model = Path(__file__).resolve().parents[1] / "models/robomaster-armor-yolov5.onnx"
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", default="rtmp://127.0.0.1:1935/live/drone")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--model", type=Path, default=default_model)
    parser.add_argument("--fps", type=float, default=8.0)
    parser.add_argument("--confidence", type=float, default=0.8)
    parser.add_argument("--nms", type=float, default=0.3)
    args = parser.parse_args()
    run(args.source, args.output, args.model, args.fps, args.confidence, args.nms)


if __name__ == "__main__":
    main()
