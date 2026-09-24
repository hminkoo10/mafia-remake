"""딜러 영상의 프레임별 노출 흔들림을 없앤다.

생성 모델은 첫 몇 프레임 동안 밝기가 서서히 올라가고(기준 이미지에서 자기 화풍으로 옮겨 가며),
손이 움직일 때도 전체 밝기가 출렁인다. 카메라와 조명은 고정이므로, 움직이지 않는 배경(화면 위쪽)의
평균 밝기를 모든 프레임에서 같은 값으로 맞춘다. 대기·딜링·오픈 영상을 같은 목표값으로 맞추면
영상을 바꿔 겹칠 때 밝기가 튀지 않는다.

사용: python dealer-normalize-exposure.py <입력> <출력.mkv> <목표 배경 밝기(0-255)> [시작 프레임] [끝 프레임]
배경 밝기는 화면 위쪽 35%의 휘도 평균이다. 목표값은 기준 프레임(예: 대기 영상 30번)에서 재 둔다.
"""
import os
import subprocess
import sys

import numpy as np

# process-dealer-clip.sh와 같은 ffmpeg (FFMPEG 환경 변수로 바꾼다).
FF = os.environ.get("FFMPEG", r"C:/temp/casino-local-video/media-deps/imageio_ffmpeg/binaries/ffmpeg-win-x86_64-v7.1.exe")


def probe(path):
    out = subprocess.run([FF, "-hide_banner", "-i", path], capture_output=True, text=True, encoding="utf-8", errors="replace").stderr
    for line in out.splitlines():
        if "Video:" in line:
            for part in line.split(","):
                part = part.strip().split(" ")[0]
                if "x" in part and part.replace("x", "").isdigit():
                    w, h = part.split("x")
                    return int(w), int(h)
    raise SystemExit("크기를 읽지 못했습니다")


def main():
    src, dst, target = sys.argv[1], sys.argv[2], float(sys.argv[3])
    start = int(sys.argv[4]) if len(sys.argv) > 4 else 0
    end = int(sys.argv[5]) if len(sys.argv) > 5 else 10**9
    w, h = probe(src)
    frame_bytes = w * h * 3
    reader = subprocess.Popen([FF, "-v", "error", "-i", src, "-f", "rawvideo", "-pix_fmt", "rgb24", "-"], stdout=subprocess.PIPE)
    writer = subprocess.Popen([FF, "-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgb24", "-s", f"{w}x{h}", "-r", "24", "-i", "-",
                               "-c:v", "ffv1", dst], stdin=subprocess.PIPE)
    top = int(h * 0.35)
    index = 0
    gains = []
    while True:
        raw = reader.stdout.read(frame_bytes)
        if len(raw) < frame_bytes:
            break
        if start <= index < end:
            frame = np.frombuffer(raw, dtype=np.uint8).reshape(h, w, 3).astype(np.float32)
            luma = 0.299 * frame[:top, :, 0] + 0.587 * frame[:top, :, 1] + 0.114 * frame[:top, :, 2]
            gain = target / max(float(luma.mean()), 1.0)
            gains.append(gain)
            # 감마 공간에서 곱하면 밝은 부분이 먼저 넘친다. 대략 선형 공간에서 곱한다.
            linear = (frame / 255.0) ** 2.2 * (gain ** 2.2)
            out = np.clip(linear ** (1 / 2.2) * 255.0 + 0.5, 0, 255).astype(np.uint8)
            writer.stdin.write(out.tobytes())
        index += 1
    writer.stdin.close()
    writer.wait()
    reader.wait()
    if gains:
        print(f"frames={len(gains)} gain min={min(gains):.3f} max={max(gains):.3f}")


main()
