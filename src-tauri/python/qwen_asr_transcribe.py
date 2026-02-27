#!/usr/bin/env python3
import argparse
import importlib
import sys


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Transcribe an audio file using Qwen ASR"
    )
    parser.add_argument("--audio", help="Path to WAV/PCM input audio")
    parser.add_argument("--model", required=True, help="Hugging Face model id")
    parser.add_argument("--language", default="auto", help="Language name or auto")
    parser.add_argument(
        "--warmup",
        action="store_true",
        help="Only load model and exit to pre-download/check runtime",
    )
    args = parser.parse_args()

    if not args.warmup and not args.audio:
        parser.error("--audio is required unless --warmup is used")

    return args


def main() -> int:
    args = parse_args()

    try:
        torch = importlib.import_module("torch")
        qwen_asr = importlib.import_module("qwen_asr")
        qwen_model = getattr(qwen_asr, "Qwen3ASRModel", None)
    except Exception as exc:
        print(
            "Missing dependencies. Install with: pip install -U qwen-asr torch torchvision",
            file=sys.stderr,
        )
        print(str(exc), file=sys.stderr)
        return 2

    use_cuda = torch.cuda.is_available()
    dtype = torch.float16 if use_cuda else torch.float32
    device_map = "cuda:0" if use_cuda else "cpu"

    try:
        if qwen_model is None:
            raise RuntimeError("qwen_asr.Qwen3ASRModel is unavailable")

        model = qwen_model.from_pretrained(
            args.model,
            dtype=dtype,
            device_map=device_map,
            max_new_tokens=768,
        )

        if args.warmup:
            print("READY")
            return 0

        language = None if args.language.lower() == "auto" else args.language
        results = model.transcribe(audio=args.audio, language=language)
        text = results[0].text.strip() if results else ""
        print(text)
        return 0
    except Exception as exc:
        print(f"Transcription failed: {exc}", file=sys.stderr)
        return 3


if __name__ == "__main__":
    raise SystemExit(main())
