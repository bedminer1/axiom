import argparse
import json
import sys
from pathlib import Path

import numpy as np
from PIL import Image


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--image", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--prompt", default="crack in concrete")
    args = parser.parse_args()

    try:
        import torch
        from sam3.model_builder import build_sam3_image_model
        from sam3.model.sam3_image_processor import Sam3Processor
    except Exception as exc:
        raise RuntimeError(f"SAM 3 is not installed correctly: {exc}") from exc

    if not torch.cuda.is_available():
        raise RuntimeError("official SAM 3 inference requires a CUDA GPU; configure AXIOM_SAM_COMMAND to point at a CUDA worker")

    image = Image.open(args.image).convert("RGB")
    model = build_sam3_image_model()
    processor = Sam3Processor(model)
    state = processor.set_image(image)
    result = processor.set_text_prompt(state=state, prompt=args.prompt)
    masks = result["masks"].detach().float().cpu().numpy()
    scores = result["scores"].detach().float().cpu().numpy().reshape(-1)
    if masks.size == 0:
        raise RuntimeError("SAM 3 found no crack instances")
    keep = scores >= 0.35
    if not np.any(keep):
        keep[np.argmax(scores)] = True
    selected = masks[keep]
    while selected.ndim > 3:
        selected = selected[:, 0]
    merged = np.any(selected > 0.5, axis=0).astype(np.uint8) * 255
    output = Path(args.output).resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    Image.fromarray(merged).resize(image.size, Image.Resampling.NEAREST).save(output)
    print(json.dumps({"mask_path": str(output), "score": float(scores[keep].max()), "instances": int(keep.sum()), "model": "Meta SAM 3 / text prompt"}))


if __name__ == "__main__":
    try:
        main()
    except Exception as exc:
        print(str(exc), file=sys.stderr)
        raise SystemExit(1)

