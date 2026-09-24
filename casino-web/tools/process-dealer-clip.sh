#!/usr/bin/env bash
# 딜러 영상 후처리 (생성한 원본 → 웹에 넣을 webm/mp4/포스터).
#   1) 노출 맞추기: 쓸 구간만 잘라 프레임마다 고정된 배경 밝기를 목표값으로 맞춘다 (dealer-normalize-exposure.py)
#   2) 삼각대 안정화: 카메라가 고정이므로 배경 기준으로 흔들림을 없앤다 (vidstab tripod)
#   3) 시간 방향 잡음 제거 (hqdn3d, 공간은 건드리지 않음) → 톤 곡선 → 1536x1024 lanczos 확대 → CAS 선명도
#   4) loop 모드: 앞으로 재생한 뒤 거꾸로 이어 붙인다 (끝점 프레임은 한 번만)
#   5) 법정 범위로 자르고 VP9(CRF 24)·H.264(CRF 18) 인코딩, 첫 프레임을 포스터로
# 사용 (출력 경로는 상대 경로로: ffmpeg 필터 인자에 C: 같은 콜론이 들어가면 깨진다):
#   TONE="curves=master='...'" process-dealer-clip.sh <입력> <출력 이름> <loop|once> <시작 프레임> <끝 프레임> <목표 배경 밝기>
# TONE은 dealer-luma-curve.py로 기준 프레임과 원본 사진에서 만든다.
set -euo pipefail
IN="$1"; OUT="$2"; MODE="$3"; START="$4"; END="$5"; TARGET="$6"
HERE="$(cd "$(dirname "$0")" && pwd)"
FF="${FFMPEG:-C:/temp/casino-local-video/media-deps/imageio_ffmpeg/binaries/ffmpeg-win-x86_64-v7.1.exe}"
TONE="${TONE:-curves=master='0/0 0.25/0.2 0.5/0.43 0.75/0.68 0.9/0.82 1/0.91'}"
WORK="work-$(basename "$OUT")"
rm -rf "$WORK"; mkdir -p "$WORK"

python "$HERE/dealer-normalize-exposure.py" "$IN" "$WORK/norm.mkv" "$TARGET" "$START" "$END"
"$FF" -hide_banner -loglevel error -y -i "$WORK/norm.mkv" -vf "vidstabdetect=tripod=1:shakiness=5:accuracy=15:stepsize=4:result=$WORK/t.trf" -f null -
STAB="vidstabtransform=tripod=1:input=$WORK/t.trf:interpol=bicubic:optzoom=0:zoom=1.2:crop=keep"
"$FF" -hide_banner -loglevel error -y -i "$WORK/norm.mkv" -vf "$STAB,hqdn3d=0:0:4:4,$TONE,eq=saturation=0.95,scale=1536:1024:flags=lanczos,cas=0.4" -c:v ffv1 "$WORK/up.mkv"

if [ "$MODE" = "loop" ]; then
  "$FF" -hide_banner -loglevel error -y -i "$WORK/up.mkv" -filter_complex \
    "[0:v]split[f][r];[r]reverse,trim=start_frame=1,setpts=PTS-STARTPTS[rv0];[rv0]reverse,trim=start_frame=1,reverse,setpts=PTS-STARTPTS[rv];[f][rv]concat=n=2:v=1,fps=24[out]" \
    -map "[out]" -c:v ffv1 "$WORK/final.mkv"
else
  cp "$WORK/up.mkv" "$WORK/final.mkv"
fi

CLAMP="format=yuv420p,lutyuv=y='clip(val,16,235)':u='clip(val,16,240)':v='clip(val,16,240)'"
"$FF" -hide_banner -loglevel error -y -i "$WORK/final.mkv" -vf "$CLAMP" -c:v libvpx-vp9 -crf 24 -b:v 0 -g 240 -row-mt 1 -deadline good -cpu-used 1 -an "$OUT.webm"
"$FF" -hide_banner -loglevel error -y -i "$WORK/final.mkv" -vf "$CLAMP" -c:v libx264 -crf 18 -preset slow -tune film -g 240 -pix_fmt yuv420p -movflags +faststart -an "$OUT.mp4"
"$FF" -hide_banner -loglevel error -y -i "$WORK/final.mkv" -vf "select=eq(n\,0)" -frames:v 1 -q:v 2 "$OUT.jpg"
ls -la "$OUT.webm" "$OUT.mp4" "$OUT.jpg"
