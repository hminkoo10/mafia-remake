"""Luma-only histogram match from a clip frame to the reference photo, emitted as an
ffmpeg `curves=master=...` string (one curve for all channels, so hue is kept).
usage: python luma_curve.py frame.png photo.png [strength=0.8]"""
import sys
import numpy as np
from PIL import Image


def luma(path, size):
    a = np.asarray(Image.open(path).convert("RGB").resize(size, Image.LANCZOS)).astype(np.float64)
    return np.clip(0.299 * a[..., 0] + 0.587 * a[..., 1] + 0.114 * a[..., 2], 0, 255).astype(np.uint8).ravel()


frame, photo = sys.argv[1], sys.argv[2]
strength = float(sys.argv[3]) if len(sys.argv) > 3 else 0.8
src = luma(frame, (576, 384))
ref = luma(photo, (576, 384))
cs = np.cumsum(np.bincount(src, minlength=256)) / src.size
cr = np.cumsum(np.bincount(ref, minlength=256)) / ref.size
mapped = np.interp(cs, cr, np.arange(256))
mapped = np.convolve(np.pad(mapped, 6, mode="edge"), np.ones(13) / 13, mode="valid")
mapped = np.maximum.accumulate(mapped)
curve = np.arange(256) * (1 - strength) + mapped * strength
points = [0, 16, 32, 64, 96, 128, 160, 192, 224, 240, 255]
print("curves=master='" + " ".join(f"{p/255:.3f}/{min(curve[p], 255)/255:.3f}" for p in points) + "'")
