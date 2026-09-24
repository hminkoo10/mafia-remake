"""딜러 영상을 기준 장면(plate) 위에 합성해 선명하고 흔들리지 않게 한다.

생성 모델은 첫 프레임(기준 장면)은 선명하지만 뒤로 갈수록 화면 전체를 조금씩 다시 그려
흐려지고(선명도 1/2~1/3), 살색이 빠지고(채도 0.52 → 0.43), 배경까지 일렁인다.
카메라는 고정이므로 움직이지 않는 곳(배경·테이블·슈·몸통)은 모든 프레임에 기준 장면을 그대로 쓰고,
실제로 움직이는 곳(손·팔·고개·눈)만 생성 프레임에서 가져온다.

  - 딜러가 움직이는 영역(머리·몸통, 팔·손·슈·테이블 앞쪽)만 생성 프레임을 쓰고 경계는 넓게 섞는다.
    (기준 장면과 다른 곳을 찾는 방식은 생성 프레임이 배경까지 다시 그려 배경을 끌고 왔다.)
  - 영역 안의 색·밝기는 기준 장면의 같은 영역에 맞춘다 (채널별 평균·표준편차, 시간으로 부드럽게).

사용: python dealer-plate-composite.py <입력.mkv> <기준 장면.png> <출력.mkv> <목표 배경 밝기>
입력은 dealer-normalize-exposure.py의 출력(배경 밝기를 맞춘 프레임)이고, 기준 장면도 같은 밝기로 맞춘다.
"""
import os
import subprocess
import sys

import numpy as np
from PIL import Image
from scipy import ndimage

FF = os.environ.get("FFMPEG", r"C:/temp/casino-local-video/media-deps/imageio_ffmpeg/binaries/ffmpeg-win-x86_64-v7.1.exe")
# 생성된 부분에 더하는 언샵 마스크 (세기, 반경 px).
SHARPEN = float(os.environ.get("PLATE_SHARPEN", "0.6"))
SHARPEN_RADIUS = 1.4


def probe(path):
    out = subprocess.run([FF, "-hide_banner", "-i", path], capture_output=True, text=True, encoding="utf-8", errors="replace").stderr
    for token in out.split():
        if "x" in token:
            a, _, b = token.rstrip(",").partition("x")
            if a.isdigit() and b.isdigit() and int(a) > 100:
                return int(a), int(b)
    raise SystemExit(f"cannot read size of {path}")


def read_frames(path, w, h):
    raw = subprocess.run([FF, "-v", "error", "-i", path, "-f", "rawvideo", "-pix_fmt", "rgb24", "-"], capture_output=True).stdout
    return np.frombuffer(raw, np.uint8).reshape(-1, h, w, 3).astype(np.float32)


def luma(img):
    return img @ np.array([0.299, 0.587, 0.114], np.float32)


def background_luma(img):
    return luma(img[: int(img.shape[0] * 0.35)]).mean()


def to_target(img, target):
    """배경(위쪽 35%) 밝기를 목표값으로 (대략 선형 공간에서 곱한다: dealer-normalize-exposure.py와 같은 방식)."""
    lin = (img / 255.0) ** 2.2
    gain = (target / max(background_luma(img), 1e-3)) ** 2.2
    return np.clip((lin * gain) ** (1 / 2.2) * 255.0, 0, 255)


def dealer_region(w, h):
    """딜러가 움직이는 영역 (1이면 생성 프레임, 0이면 기준 장면). 1152x768 기준 좌표를 크기에 맞춘다.
    머리·몸통 기둥 + 팔·손·슈·테이블 앞쪽 띠. 위쪽 배경·기둥·조명·양옆 의자는 늘 기준 장면이다."""
    sx, sy = w / 1152, h / 768
    mask = np.zeros((h, w), np.float32)
    mask[0: int(640 * sy), int(380 * sx): int(760 * sx)] = 1  # 머리·몸통
    mask[int(170 * sy): int(660 * sy), int(700 * sx): int(1010 * sx)] = 1  # 오른팔 전체 (어깨~슈, 손이 오가는 길)
    mask[int(280 * sy): int(660 * sy), int(240 * sx): int(1010 * sx)] = 1  # 왼팔·손·슈·테이블 앞
    return ndimage.gaussian_filter(mask, 22 * sx)


def eyes_region(w, h):
    """눈·눈썹 (PLATE_REGION=eyes: 대기 영상은 얼굴·몸 모두 기준 장면을 쓰고 눈 깜빡임만 생성 프레임에서 가져온다)."""
    sx, sy = w / 1152, h / 768
    mask = np.zeros((h, w), np.float32)
    mask[int(95 * sy): int(140 * sy), int(535 * sx): int(640 * sx)] = 1
    return ndimage.gaussian_filter(mask, 7 * sx)


def head_region(w, h):
    """얼굴 영역 (따로 색을 맞춘다: 생성 프레임은 얼굴이 기준 장면보다 어둡고 칙칙해진다)."""
    sx, sy = w / 1152, h / 768
    mask = np.zeros((h, w), np.float32)
    mask[int(10 * sy): int(250 * sy), int(460 * sx): int(690 * sx)] = 1
    return ndimage.gaussian_filter(mask, 18 * sx)


def match_stats(frames, plate, where):
    """where 영역의 채널별 평균·표준편차를 기준 장면에 맞춘 프레임들 (통계는 시간으로 부드럽게)."""
    p_mean, p_std = plate[where].mean(0), plate[where].std(0)
    means = ndimage.uniform_filter1d(np.array([f[where].mean(0) for f in frames]), 5, axis=0, mode="nearest")
    stds = ndimage.uniform_filter1d(np.array([f[where].std(0) + 1e-3 for f in frames]), 5, axis=0, mode="nearest")
    return [np.clip((f - m) / sd * p_std + p_mean, 0, 255) for f, m, sd in zip(frames, means, stds)]


def main():
    src, plate_path, dst, target = sys.argv[1], sys.argv[2], sys.argv[3], float(sys.argv[4])
    w, h = probe(src)
    frames = read_frames(src, w, h)
    plate = np.asarray(Image.open(plate_path).convert("RGB").resize((w, h), Image.LANCZOS), np.float32)
    plate = to_target(plate, target)
    region = eyes_region(w, h) if os.environ.get("PLATE_REGION") == "eyes" else dealer_region(w, h)
    inside = dealer_region(w, h) > 0.5

    # 영역 안의 색·밝기를 기준 장면의 같은 영역에 맞추고, 얼굴은 얼굴끼리 한 번 더 맞춘다.
    body = match_stats(frames, plate, inside)
    head = head_region(w, h)
    faces = match_stats(frames, plate, head > 0.5)
    head = head[..., None]
    matched_frames = [b * (1 - head) + f * head for b, f in zip(body, faces)]
    # 생성 프레임은 기준 장면보다 흐리다 (선명도 절반 정도): 생성된 부분에만 언샵 마스크를 더 걸어
    # 기준 장면과 이어질 때 선명했다 흐려졌다 하지 않게 한다.
    # 기준 장면만큼 선명한 프레임(첫 프레임 등)은 건드리지 않고, 흐린 만큼만 세게 건다.
    def detail(img):
        l = luma(img)[inside]
        return float(np.var(l - ndimage.gaussian_filter(luma(img), SHARPEN_RADIUS)[inside]))

    plate_detail = detail(plate)
    sharpened = []
    for f in matched_frames:
        amount = SHARPEN * float(np.clip(2 * (1 - detail(f) / plate_detail), 0, 1))
        blur = ndimage.gaussian_filter(f, (SHARPEN_RADIUS, SHARPEN_RADIUS, 0))
        sharpened.append(np.clip(f + amount * (f - blur), 0, 255))
    matched_frames = sharpened

    enc = subprocess.Popen(
        [FF, "-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgb24", "-s", f"{w}x{h}", "-r", "24", "-i", "-", "-c:v", "ffv1", dst],
        stdin=subprocess.PIPE,
    )
    alpha = region[..., None]
    for matched in matched_frames:
        out = matched * alpha + plate * (1 - alpha)
        enc.stdin.write(np.clip(out + 0.5, 0, 255).astype(np.uint8).tobytes())
    enc.stdin.close()
    enc.wait()
    print(f"frames={len(frames)} region={float(region.mean()):.2f} of the frame")


if __name__ == "__main__":
    main()
