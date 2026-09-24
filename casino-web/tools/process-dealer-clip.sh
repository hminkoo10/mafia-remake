#!/usr/bin/env bash
# 딜러 영상 후처리 (생성한 원본 → 웹에 넣을 webm/mp4/포스터).
#   1) 노출 맞추기: 쓸 구간만 잘라 프레임마다 고정된 배경 밝기를 목표값으로 맞춘다 (dealer-normalize-exposure.py)
#   2) 삼각대 안정화: 카메라가 고정이므로 배경 기준으로 흔들림을 없앤다 (vidstab tripod)
#   3) 시간 방향 잡음 제거 (hqdn3d, 공간은 건드리지 않음) → 톤 곡선 → 1536x1024 lanczos 확대 → CAS 선명도
#   4) loop 모드: 앞으로 재생한 뒤 거꾸로 이어 붙인다 (끝점 프레임은 한 번만)
#      settle 모드: 앞으로만 재생하고 마지막 SETTLE 프레임(기본 12) 동안 첫 프레임으로 서서히 섞는다.
#        끝이 첫 프레임과 같아져 반복해도 이음새가 없다 (눈 깜빡임 같은 동작이 거꾸로 나오지 않는다).
#      reverse 모드: 거꾸로 뒤집는다 (대기 자세에서 시작해 생성한 동작을 대기 자세로 끝나는 동작으로 쓸 때).
#   5) 법정 범위로 자르고 VP9(CRF 24)·H.264(CRF 18) 인코딩, 첫 프레임을 포스터로
# 사용 (출력 경로는 상대 경로로: ffmpeg 필터 인자에 C: 같은 콜론이 들어가면 깨진다):
#   TONE="curves=master='...'" process-dealer-clip.sh <입력> <출력 이름> <loop|once|settle|reverse> <시작 프레임> <끝 프레임> <목표 배경 밝기>
# TONE은 dealer-luma-curve.py로 기준 프레임과 원본 사진에서 만든다.
set -euo pipefail
IN="$1"; OUT="$2"; MODE="$3"; START="$4"; END="$5"; TARGET="$6"
HERE="$(cd "$(dirname "$0")" && pwd)"
FF="${FFMPEG:-C:/temp/casino-local-video/media-deps/imageio_ffmpeg/binaries/ffmpeg-win-x86_64-v7.1.exe}"
TONE="${TONE:-curves=master='0/0 0.25/0.2 0.5/0.43 0.75/0.68 0.9/0.82 1/0.91'}"
WORK="work-$(basename "$OUT")"
rm -rf "$WORK"; mkdir -p "$WORK"

python "$HERE/dealer-normalize-exposure.py" "$IN" "$WORK/norm.mkv" "$TARGET" "$START" "$END"
if [ -n "${PLATE:-}" ]; then
  # PLATE=<생성에 쓴 기준 장면 PNG>: 움직이지 않는 곳은 기준 장면을 그대로 쓰고 움직이는 곳만 생성 프레임에서
  # 가져온다 (dealer-plate-composite.py). 배경이 고정되므로 vidstab은 쓰지 않는다.
  python "$HERE/dealer-plate-composite.py" "$WORK/norm.mkv" "$PLATE" "$WORK/comp.mkv" "$TARGET"
  "$FF" -hide_banner -loglevel error -y -i "$WORK/comp.mkv" -vf "hqdn3d=0:0:3:3,$TONE,scale=1536:1024:flags=lanczos,cas=0.5" -c:v ffv1 "$WORK/up.mkv"
else
  "$FF" -hide_banner -loglevel error -y -i "$WORK/norm.mkv" -vf "vidstabdetect=tripod=1:shakiness=5:accuracy=15:stepsize=4:result=$WORK/t.trf" -f null -
  STAB="vidstabtransform=tripod=1:input=$WORK/t.trf:interpol=bicubic:optzoom=0:zoom=1.2:crop=keep"
  "$FF" -hide_banner -loglevel error -y -i "$WORK/norm.mkv" -vf "$STAB,hqdn3d=0:0:4:4,$TONE,eq=saturation=0.95,scale=1536:1024:flags=lanczos,cas=0.4" -c:v ffv1 "$WORK/up.mkv"
fi

if [ "$MODE" = "loop" ]; then
  "$FF" -hide_banner -loglevel error -y -i "$WORK/up.mkv" -filter_complex \
    "[0:v]split[f][r];[r]reverse,trim=start_frame=1,setpts=PTS-STARTPTS[rv0];[rv0]reverse,trim=start_frame=1,reverse,setpts=PTS-STARTPTS[rv];[f][rv]concat=n=2:v=1,fps=24[out]" \
    -map "[out]" -c:v ffv1 "$WORK/final.mkv"
elif [ "$MODE" = "settle" ]; then
  K="${SETTLE:-12}"
  N=$(( END - START ))
  OFF=$(python -c "print(($N - $K) / 24)")
  DUR=$(python -c "print($K / 24)")
  "$FF" -hide_banner -loglevel error -y -i "$WORK/up.mkv" -filter_complex \
    "[0:v]fps=24,settb=AVTB,split[a][b];[b]trim=end_frame=1,loop=loop=$(( K - 1 )):size=1:start=0,setpts=N/24/TB,fps=24,settb=AVTB[still];[a][still]xfade=transition=fade:duration=$DUR:offset=$OFF[out]" \
    -map "[out]" -c:v ffv1 "$WORK/final.mkv"
elif [ "$MODE" = "reverse" ]; then
  "$FF" -hide_banner -loglevel error -y -i "$WORK/up.mkv" -vf "reverse,setpts=N/24/TB,fps=24" -c:v ffv1 "$WORK/final.mkv"
else
  cp "$WORK/up.mkv" "$WORK/final.mkv"
fi

# HOLD=<프레임 수>: 끝에 첫 프레임(기준 장면)을 그만큼 이어 붙인다 (대기 영상: 깜빡임 사이를 길게).
if [ -n "${HOLD:-}" ]; then

  "$FF" -hide_banner -loglevel error -y -i "$WORK/final.mkv" -filter_complex \
    "[0:v]fps=24,settb=AVTB,split[a][b];[b]trim=end_frame=1,loop=loop=$(( HOLD - 1 )):size=1:start=0,setpts=N/24/TB,fps=24,settb=AVTB[still];[a][still]concat=n=2:v=1[out]" \
    -map "[out]" -c:v ffv1 "$WORK/final-hold.mkv"
  mv "$WORK/final-hold.mkv" "$WORK/final.mkv"
fi

# LEADIN=<앞 영상의 마지막 프레임 PNG>: 처음 LEADIN_FRAMES(기본 8) 프레임 동안 그 장면에서 서서히 넘어온다.
# 앞 영상에서 이어지는 영상(딜 → 되돌리기)의 이음새를 같은 장면에서 시작하게 한다.
if [ -n "${LEADIN:-}" ]; then
  LK="${LEADIN_FRAMES:-8}"
  LDUR=$(python -c "print($LK / 24)")
  "$FF" -hide_banner -loglevel error -y -loop 1 -framerate 24 -t "$LDUR" -i "$LEADIN" -i "$WORK/final.mkv" -filter_complex \
    "[0:v]scale=1536:1024,format=yuv444p,fps=24,settb=AVTB[lead];[1:v]fps=24,settb=AVTB[clip];[lead][clip]xfade=transition=fade:duration=$LDUR:offset=0[out]" \
    -map "[out]" -c:v ffv1 "$WORK/final-lead.mkv"
  mv "$WORK/final-lead.mkv" "$WORK/final.mkv"
fi

CLAMP="format=yuv420p,lutyuv=y='clip(val,16,235)':u='clip(val,16,240)':v='clip(val,16,240)'"
"$FF" -hide_banner -loglevel error -y -i "$WORK/final.mkv" -vf "$CLAMP" -c:v libvpx-vp9 -crf 24 -b:v 0 -g 240 -row-mt 1 -deadline good -cpu-used 1 -an "$OUT.webm"
"$FF" -hide_banner -loglevel error -y -i "$WORK/final.mkv" -vf "$CLAMP" -c:v libx264 -crf 18 -preset slow -tune film -g 240 -pix_fmt yuv420p -movflags +faststart -an "$OUT.mp4"
"$FF" -hide_banner -loglevel error -y -i "$WORK/final.mkv" -vf "select=eq(n\,0)" -frames:v 1 -q:v 2 "$OUT.jpg"
ls -la "$OUT.webm" "$OUT.mp4" "$OUT.jpg"
