"""Builds the tiny random Parakeet TDT that crates/dsp checks its speech code against.

Run with transformers and torch installed:
    python scripts/speech-fixture.py <dir>
    cargo xtask gen speech-model --from <dir> --out crates/dsp/src/audio/speech/fixtures/tiny.sdrmmnn
then copy <dir>/expected.json beside it.
"""

import json
import sys
from pathlib import Path

import numpy as np
import torch
from transformers import ParakeetFeatureExtractor, ParakeetForTDT, ParakeetTDTConfig

RATE = 16000
WORD = chr(0x2581)
PIECES = ["<unk>", "<|endoftext|>", WORD, "a", "e", "i", "o", WORD + "d", "er", "s", "t", "n", "<0xC3>", "<0x9F>", ".", ","]


def audio():
    n = np.arange(RATE, dtype=np.float64)
    sweep = 0.3 * np.sin(2 * np.pi * (200 + 150 * n / RATE) * n / RATE)
    tone = 0.1 * np.sin(2 * np.pi * 1234 * n / RATE)
    return (sweep + tone).astype(np.float32)


def main(out):
    out.mkdir(parents=True, exist_ok=True)
    torch.manual_seed(7)
    config = ParakeetTDTConfig(
        encoder_config={
            "hidden_size": 32,
            "num_attention_heads": 2,
            "num_key_value_heads": 2,
            "num_hidden_layers": 2,
            "intermediate_size": 64,
            "conv_kernel_size": 9,
            "subsampling_conv_channels": 8,
            "subsampling_factor": 8,
            "num_mel_bins": 128,
            "initializer_range": 0.2,
            "scale_input": False,
            "attention_bias": False,
            "convolution_bias": False,
        },
        vocab_size=len(PIECES) + 1,
        blank_token_id=len(PIECES),
        decoder_hidden_size=12,
        num_decoder_layers=2,
        durations=[0, 1, 2, 3, 4],
        initializer_range=0.5,
    )
    model = ParakeetForTDT(config).eval()
    with torch.no_grad():
        for name, param in model.named_parameters():
            if "norm" in name and name.endswith("weight"):
                param.uniform_(0.5, 1.5)
            elif param.dim() == 1:
                param.normal_(0, 0.1)
        for module in model.modules():
            if isinstance(module, torch.nn.BatchNorm1d):
                module.running_mean.normal_(0, 0.1)
                module.running_var.uniform_(0.5, 1.5)
    model.save_pretrained(out)
    extractor = ParakeetFeatureExtractor(feature_size=128)
    (out / "processor_config.json").write_text(json.dumps({"feature_extractor": extractor.to_dict()}))
    vocab = {piece: index for index, piece in enumerate(PIECES)}
    added = [{"id": 0, "content": "<unk>", "special": True}, {"id": 1, "content": "<|endoftext|>", "special": False}]
    (out / "tokenizer.json").write_text(json.dumps({"model": {"type": "BPE", "vocab": vocab}, "added_tokens": added}))
    inputs = extractor(audio(), sampling_rate=RATE, return_tensors="pt")
    with torch.no_grad():
        encoded = model.get_audio_features(input_features=inputs.input_features, attention_mask=inputs.attention_mask)
        outputs = config.vocab_size + len(config.durations)
        generated = model.generate(
            **inputs,
            decoder_start_token_id=config.blank_token_id,
            suppress_tokens=list(range(config.vocab_size, outputs)),
        )
    tokens = [t for t in generated.sequences[0].tolist()[1:] if t != config.blank_token_id]
    (out / "expected.json").write_text(json.dumps({
        "encoded": encoded.last_hidden_state[0].flatten().tolist(),
        "frames": encoded.last_hidden_state.shape[1],
        "tokens": tokens,
    }))
    print("frames", encoded.last_hidden_state.shape[1], "tokens", tokens)


if __name__ == "__main__":
    main(Path(sys.argv[1]))
