import argparse
import json
import sys
from collections import deque
from pathlib import Path

import numpy as np
import torch
import torch.nn as nn
import torch.nn.functional as F
from huggingface_hub import hf_hub_download
from PIL import Image

MODEL_REPOSITORY = "samir-mohamed/concrete-crack-segmentation"
MODEL_FILENAME = "unet_model_weights.pth"


class ImprovedUNet(nn.Module):
    """Architecture published with the concrete-crack checkpoint."""

    def __init__(self, in_channels: int = 3, out_channels: int = 1, depth: int = 4, start_filters: int = 64):
        super().__init__()

        def conv_block(in_ch: int, out_ch: int, dropout: float = 0.2) -> nn.Sequential:
            return nn.Sequential(
                nn.Conv2d(in_ch, out_ch, kernel_size=3, padding=1),
                nn.BatchNorm2d(out_ch),
                nn.ReLU(inplace=True),
                nn.Dropout2d(p=dropout),
                nn.Conv2d(out_ch, out_ch, kernel_size=3, padding=1),
                nn.BatchNorm2d(out_ch),
                nn.ReLU(inplace=True),
                nn.Dropout2d(p=dropout),
            )

        self.encoders = nn.ModuleList()
        self.pools = nn.ModuleList()
        in_ch = in_channels
        for level in range(depth):
            out_ch = start_filters * (2**level)
            self.encoders.append(conv_block(in_ch, out_ch))
            self.pools.append(nn.MaxPool2d(2))
            in_ch = out_ch

        bottleneck_ch = start_filters * (2**depth)
        self.bottleneck = conv_block(in_ch, bottleneck_ch, dropout=0.3)
        self.decoders = nn.ModuleList()
        self.upconvs = nn.ModuleList()
        for level in range(depth - 1, -1, -1):
            out_ch = start_filters * (2**level)
            decoder_in = bottleneck_ch if level == depth - 1 else start_filters * (2 ** (level + 1))
            self.upconvs.append(nn.ConvTranspose2d(decoder_in, out_ch, kernel_size=2, stride=2))
            self.decoders.append(conv_block(decoder_in, out_ch))
            bottleneck_ch = out_ch

        self.final_conv = nn.Conv2d(start_filters, out_channels, kernel_size=1)

    def forward(self, value: torch.Tensor) -> torch.Tensor:
        skips = []
        for encoder, pool in zip(self.encoders, self.pools):
            value = encoder(value)
            skips.append(value)
            value = pool(value)
        value = self.bottleneck(value)
        for index, (upconv, decoder) in enumerate(zip(self.upconvs, self.decoders)):
            value = upconv(value)
            skip = skips[-(index + 1)]
            if value.shape[2:] != skip.shape[2:]:
                value = F.interpolate(value, size=skip.shape[2:], mode="bilinear", align_corners=False)
            value = decoder(torch.cat([value, skip], dim=1))
        return torch.sigmoid(self.final_conv(value))


def preprocess_batch(tiles: list[np.ndarray]) -> torch.Tensor:
    value = torch.from_numpy(np.stack(tiles).astype(np.float32)).permute(0, 3, 1, 2) / 255.0
    mean = torch.tensor([0.485, 0.456, 0.406]).view(3, 1, 1)
    std = torch.tensor([0.229, 0.224, 0.225]).view(3, 1, 1)
    return (value - mean) / std


def tile_starts(length: int, tile_size: int, overlap: int) -> list[int]:
    if length <= tile_size:
        return [0]
    starts = list(range(0, length - tile_size + 1, tile_size - overlap))
    if starts[-1] != length - tile_size:
        starts.append(length - tile_size)
    return starts


def tiled_probability(
    model: nn.Module,
    image: Image.Image,
    device: torch.device,
    tile_size: int = 256,
    overlap: int = 64,
    batch_size: int = 4,
) -> np.ndarray:
    original = np.asarray(image, dtype=np.uint8)
    height, width = original.shape[:2]
    padded = np.pad(
        original,
        ((0, max(0, tile_size - height)), (0, max(0, tile_size - width)), (0, 0)),
        mode="edge",
    )
    padded_height, padded_width = padded.shape[:2]
    coordinates = [
        (y, x)
        for y in tile_starts(padded_height, tile_size, overlap)
        for x in tile_starts(padded_width, tile_size, overlap)
    ]
    probability_sum = np.zeros((padded_height, padded_width), dtype=np.float32)
    weight_sum = np.zeros_like(probability_sum)
    axis_weight = np.clip(np.hanning(tile_size + 2)[1:-1], 0.05, None)
    tile_weight = np.outer(axis_weight, axis_weight).astype(np.float32)

    with torch.inference_mode():
        for offset in range(0, len(coordinates), batch_size):
            batch_coordinates = coordinates[offset : offset + batch_size]
            tiles = [padded[y : y + tile_size, x : x + tile_size] for y, x in batch_coordinates]
            background = model(preprocess_batch(tiles).to(device)).squeeze(1).cpu().numpy()
            crack = 1.0 - background
            for prediction, (y, x) in zip(crack, batch_coordinates):
                probability_sum[y : y + tile_size, x : x + tile_size] += prediction * tile_weight
                weight_sum[y : y + tile_size, x : x + tile_size] += tile_weight

    return (probability_sum / weight_sum)[:height, :width]


def hysteresis_mask(
    probability: np.ndarray,
    low_threshold: float,
    high_threshold: float,
    min_component_pixels: int,
) -> tuple[np.ndarray, int]:
    """Keep low-confidence pixels only in components containing a strong seed."""
    candidates = probability >= low_threshold
    seeds = probability >= high_threshold
    visited = np.zeros(candidates.shape, dtype=bool)
    selected = np.zeros(candidates.shape, dtype=bool)
    height, width = candidates.shape
    instances = 0

    for start_y, start_x in np.argwhere(candidates):
        if visited[start_y, start_x]:
            continue
        queue = deque([(int(start_y), int(start_x))])
        visited[start_y, start_x] = True
        component: list[tuple[int, int]] = []
        has_seed = False
        while queue:
            y, x = queue.popleft()
            component.append((y, x))
            has_seed = has_seed or bool(seeds[y, x])
            for next_y in range(max(0, y - 1), min(height, y + 2)):
                for next_x in range(max(0, x - 1), min(width, x + 2)):
                    if candidates[next_y, next_x] and not visited[next_y, next_x]:
                        visited[next_y, next_x] = True
                        queue.append((next_y, next_x))
        if has_seed and len(component) >= min_component_pixels:
            instances += 1
            rows, columns = zip(*component)
            selected[rows, columns] = True

    return selected, instances


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--image", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--low-threshold", type=float, default=0.10)
    parser.add_argument("--high-threshold", type=float, default=0.35)
    parser.add_argument("--threshold", type=float, help="deprecated alias for --high-threshold")
    parser.add_argument("--overlap", type=int, default=64)
    parser.add_argument("--min-component-pixels", type=int, default=128)
    args = parser.parse_args()

    device = torch.device("cuda" if torch.cuda.is_available() else "cpu")
    checkpoint = hf_hub_download(repo_id=MODEL_REPOSITORY, filename=MODEL_FILENAME)
    model = ImprovedUNet()
    model.load_state_dict(torch.load(checkpoint, map_location=device, weights_only=True))
    model.to(device).eval()

    high_threshold = args.threshold if args.threshold is not None else args.high_threshold
    if not 0.0 <= args.low_threshold < high_threshold <= 1.0:
        raise ValueError("thresholds must satisfy 0 <= low-threshold < high-threshold <= 1")
    if not 0 <= args.overlap < 256:
        raise ValueError("overlap must be between 0 and 255")
    if args.min_component_pixels < 1:
        raise ValueError("min-component-pixels must be positive")

    image = Image.open(args.image).convert("RGB")
    probability = tiled_probability(model, image, device, overlap=args.overlap)
    selected, instances = hysteresis_mask(
        probability,
        low_threshold=args.low_threshold,
        high_threshold=high_threshold,
        min_component_pixels=args.min_component_pixels,
    )
    if not np.any(selected):
        raise RuntimeError("concrete crack model found no connected crack pixels above the configured thresholds")

    confidence = float(probability[selected].mean())
    mask = Image.fromarray((selected.astype(np.uint8) * 255), mode="L")
    output = Path(args.output).resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    mask.save(output)
    print(json.dumps({
        "mask_path": str(output),
        "score": confidence,
        "instances": instances,
        "model": "Samir Mohamed concrete-crack U-Net / tiled hysteresis",
    }))


if __name__ == "__main__":
    try:
        main()
    except Exception as exc:
        print(str(exc), file=sys.stderr)
        raise SystemExit(1)
