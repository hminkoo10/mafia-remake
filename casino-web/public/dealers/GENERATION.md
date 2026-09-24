# 실사 딜러 에셋 (2026-09-24)

## 새 파이프라인

기존 768×512 클립은 화질이 부드럽고 밝았다. 사진에서는 하이라이트 클리핑이 0.3%였지만 기존 클립은 5.6%였다. 화면이 약 2px 흔들렸고 텍스처 보일이 3.0이었다. 얼굴도 흔들렸다. 1152×768로 높여도 모델은 원본 사진의 얼굴을 유지하지 못하고 약 8프레임 뒤 다른 얼굴을 다시 그린다.

그래서 다음 순서로 만들었다.

1. 사진에서 1152×768 대기 영상을 만든다.
2. 얼굴이 안정되는 10~48프레임만 남긴다.
3. 30프레임을 새 기준 프레임 `anchor-hr.png`로 뽑는다.
4. 이 기준 프레임에서 1152×768 딜링 영상과 플립 영상을 만든다. 세 영상이 같은 얼굴을 공유한다.

49프레임 클립 하나를 만드는 데 이 PC에서 약 37분이 걸린다(샘플링 약 13분, VAE 디코딩 약 23분).

## 후처리

`casino-web/tools/process-dealer-clip.sh`로 처리한다.

- 프레임별 노출을 기준 프레임의 배경 luma 56.56에 맞춘다(`dealer-normalize-exposure.py`).
- vidstab tripod 안정화를 적용한다.
- temporal-only hqdn3d를 적용한다.
- `dealer-luma-curve.py`의 luma 일치 톤 커브를 strength 0.65로 적용한다.
- 채도 0.95, 1536×1024 Lanczos 확대, CAS 0.4, legal-range clamp를 적용한다.
- WebM은 VP9 CRF 24, MP4는 H.264 CRF 18로 인코딩한다.
- 포스터는 0프레임에서 만든다.

`vq/tone-curve.txt`의 정확한 커브는 다음과 같다.

```text
curves=master='0.000/0.016 0.063/0.064 0.125/0.100 0.251/0.223 0.376/0.306 0.502/0.402 0.627/0.497 0.753/0.599 0.878/0.717 0.941/0.784 1.000/0.936'
```

대기 영상은 끝 프레임을 중복하지 않는 ping-pong 루프다. 원본 39프레임이 76프레임이 되어 24fps에서 3.2초다. 딜링 영상은 0~30프레임(1.29초)을 쓴다: 12프레임에 카드가 슈에서 빠지고, 20~24프레임에 앞으로 가져와 25프레임에 펠트에 닿고, 28프레임에 내려놓는다. 처음에는 0~21프레임(꺼내기까지, 0.9초)만 썼는데 카드를 놓는 동작이 없어 2026-09-24 밤에 바꿨다 (`TONE=... process-dealer-clip.sh sophia-deal-hr.webm v3-deal once 0 31 56.56`, 배경 밝기 56.85). 웹은 카드마다 12프레임을 카드가 슈를 떠나는 순간에, 28프레임을 좌석에 놓이는 순간에 맞춰 약 1.59배속(0.667초 ÷ 비행 0.42초)으로 튼다(`schedule.ts` `dealClipAt`). 딜 영상은 두 손을 가운데 카드 위에 둔 채 끝나는데 대기 영상과 다음 딜 영상은 오른손이 슈 위에 있는 자세로 시작해, 카드마다 손이 순간 이동했다. 그래서 되돌리기 영상(`sophia-return`)을 따로 만들었다: 딜 원본 30프레임을 시작 이미지로 (`anchor-return-hr.png`) `generate-dealer-local.ps1 -Mood return -Width 1152 -Height 768 -Frames 49 -Seed 911`로 생성하고 3~36프레임(1.42초)을 같은 후처리로 쓴다(`process-dealer-clip.sh ... once 3 37 56.56`, 배경 밝기 56.36). 첫 시도(seed 473)는 손이 하얗게 뭉개져 무언가를 쥔 것처럼 보여 버렸고, 프롬프트에 빈손("empty right hand ... holding nothing")을 넣어 다시 만들었다. 웹은 카드마다 딜 영상이 끝나면 되돌리기 영상을 이어 틀고, 다음 카드의 딜 영상이 시작하기 전에 손이 슈에 닿도록 1~2.5배속으로 맞춘다. 서버의 카드 간격은 이 두 동작이 들어가는 1.25초다.

## 검수 결과

| 측정값 | 기존 대기 | 새 대기 | 새 딜링 |
|---|---:|---:|---:|
| 흔들림 (px) | 1.91 | 0.83 | 0.89 |
| 깜빡임 | 0.18 | 0.03 | 0.26 |
| 텍스처 보일 | 3.02 | 1.46 | 3.65 |
| 벽 클리핑 | 5.6% | 0.01% | - |

대기와 딜링 밝기는 44.4와 44.5로 맞았다.

플립도 생성했지만 버렸다. 플립 동작이 없고 카드 두 장이 펠트 위에 갑자기 나타났다. 플립 클립은 배포하지 않는다. 플립 중 웹은 대기 영상을 유지한다.

## 필요한 모델과 실행기

모델·실행 파일은 저장소와 서버 빌드에 넣지 않는다. 웹은 완성된 영상만 재생한다.

- 실행기: [stable-diffusion.cpp](https://github.com/leejet/stable-diffusion.cpp/releases/tag/master-899-28b454b), Windows Vulkan 빌드 `master-899-28b454b`.
- 영상 모델: [FastWan2.2-TI2V-5B Q6_K](https://huggingface.co/Green-Sky/FastWan2.2-TI2V-5B-FullAttn-GGUF), 원본 [FastVideo 모델](https://huggingface.co/FastVideo/FastWan2.2-TI2V-5B-FullAttn-Diffusers).
- 텍스트 인코더: [UMT5-XXL Q4_K_M](https://huggingface.co/city96/umt5-xxl-encoder-gguf).
- 디코더: [Wan 2.2 VAE](https://huggingface.co/Comfy-Org/Wan_2.2_ComfyUI_Repackaged/tree/main/split_files/vae).

`RuntimeRoot`의 구성:

```text
runtime/sd-cli.exe
models/FastWan2.2-TI2V-5B-q6_k.gguf
models/umt5-xxl-encoder-Q4_K_M.gguf
models/wan2.2_vae.safetensors
```

검증한 SHA-256:

| 파일 | SHA-256 |
|---|---|
| Windows Vulkan ZIP | `3a4e5a75f022e4c0cad3e5a28c5921683adcb1808c0dd8e8a3536493497c793b` |
| FastWan Q6_K | `416a87e30f2328dbefd7666ac90b395ead74f443748ff31c83483ac4ac6121cc` |
| UMT5 Q4_K_M | `17cf97a5bbbc60a646d6105b832b6f657ce904a8a1ad970e4b59df0c67584a40` |
| Wan 2.2 VAE | `e40321bd36b9709991dae2530eb4ac303dd168276980d3e9bc4b6e2b75fed156` |

이 PC: Intel Core Ultra 7 155H, Intel Arc 내장 GPU, RAM 32GB. 생성 중에는 GPU를 거의 다 쓰므로 웹 재생 확인은 생성이 끝난 뒤에 한다. 중간 결과·로그·기준 프레임은 `C:/temp/casino-local-video/`(`output/`, `anchor/`, `final/`)에 있다.

## 이음새 없는 세 영상 (2026-09-25)

사용자 지적: 눈 깜빡임이 부자연스럽고, 영상의 끝 장면과 첫 장면이 이어지지 않고 밝기도 다르다.
- 원인: 대기 영상은 ping-pong(앞으로 → 거꾸로)이라 어색한 반쯤 감는 깜빡임이 3.2초에 네 번, 그것도 거꾸로 한 번 더 나왔다. 되돌리기 영상의 끝 장면은 모델이 알아서 만든 자세라 대기·딜 영상의 첫 장면과 얼굴·밝기가 달랐다(배경 밝기만 맞추고 있었다).
- 모든 영상이 같은 대기 장면(`anchor-hr.png`)에서 시작하고 끝나게 다시 만들었다.
  - 대기: 기준 장면에서 새로 생성(`-Mood idle -Suffix -hr3`, 눈을 뜨고 한 번만 깜빡이는 프롬프트). 0~40프레임을 앞으로만 쓰고 마지막 12프레임 동안 첫 장면으로 섞는다(`process-dealer-clip.sh ... settle 0 41 56.56`, `SETTLE=12`). 웹은 0.6배속으로 틀어 깜빡임이 약 2.9초에 한 번 나온다.
  - 되돌리기: 기준 장면에서 손을 슈에서 가운데로 가져가는 영상을 생성(`-Mood reach`)해 0~25프레임을 거꾸로 쓴다(`reverse 0 26`). 끝 장면이 대기 장면과 같다. 처음 8프레임은 딜 영상의 마지막 장면에서 서서히 넘어온다(`LEADIN=<딜 마지막 프레임 PNG> LEADIN_FRAMES=8`).
  - 대기로 돌아올 때는 대기 영상을 처음부터 튼다(`chooseDealerLayer`).
- 이음새 실측(두 장면의 평균 휘도 차, 0~255): 대기 끝→처음 0.95, 되돌리기 끝→대기 1.11, 대기→딜 0.00, 딜 끝→되돌리기 1.23 (이전: 되돌리기 끝→대기 약 10, 딜 끝→되돌리기 6.85, 얼굴 밝기 86→72).

## 선명도 유지: 기준 장면 합성 (2026-09-25)

사용자 지적: 처음엔 살색이 괜찮다가 뒤로 가면 하얘지고 뿌옇고 흔들린다.
- 원인(실측): 0프레임(기준 장면)은 선명도 332·채도 0.52인데, 모델이 만든 뒤 프레임은 선명도 100~170·채도 0.42~0.45로 떨어지고 배경까지 매 프레임 다시 그려 일렁였다(배경 차이 최대 21).
- `casino-web/tools/dealer-plate-composite.py`(`process-dealer-clip.sh`의 `PLATE=<기준 장면 PNG>`): 카메라가 고정이라 딜러가 움직이는 영역(머리·몸통, 오른팔 전체, 팔·손·슈·테이블 앞)만 생성 프레임을 쓰고 나머지는 기준 장면 그대로. 영역 안은 기준 장면에 색·밝기를 맞추고(몸 전체, 얼굴은 따로), 기준 장면보다 흐린 만큼만 언샵 마스크를 건다. vidstab은 쓰지 않는다.
- 대기는 `PLATE_REGION=eyes`: 얼굴·몸 모두 기준 장면이고 눈 깜빡임만 생성 프레임에서 가져온다(0~26프레임, `SETTLE=6`, `HOLD=40`으로 뒤에 기준 장면 1.7초). 2.79초, 1배속으로 약 2.8초에 한 번 깜빡인다.
- 명령: `PLATE=anchor-hr.png process-dealer-clip.sh sophia-deal-hr.webm ... once 0 31 56.56`, `PLATE=... LEADIN=<딜 마지막 프레임> process-dealer-clip.sh sophia-reach-hr.webm ... reverse 0 26 56.56`, `PLATE=... PLATE_REGION=eyes SETTLE=6 HOLD=40 process-dealer-clip.sh sophia-idle-hr3.webm ... settle 0 27 56.56`.
- 결과(얼굴 선명도·채도, 배경 변화): 대기 475~495·0.49·0.1 이하, 딜 330~546·0.49~0.52, 되돌리기 250~480·0.47~0.54 (이전 대기 100~330·0.42~0.52, 딜 105~330, 되돌리기 50~300). 이음새 0~1.5.

## 얼굴 고정 (2026-09-25)

사용자 요청: 얼굴 생김새가 달라지지 않게. 딜·되돌리기에서 고개를 숙이는 구간은 모델이 얼굴을 새로 그려 생김새가 조금씩 달라졌다.
- `PLATE_HEAD=1`: 머리·얼굴·목(타원 + 목·옷깃)을 모든 프레임에서 기준 장면으로 고정하고, 팔·손·몸만 생성 프레임에서 가져온다. 이음새는 검은 옷깃에서 섞여 드러나지 않는다. 대신 카드를 놓을 때 고개를 숙이지 않고 카메라를 본다.
- 명령: `PLATE=anchor-hr.png PLATE_HEAD=1 process-dealer-clip.sh sophia-deal-hr.webm ... once 0 31 56.56`, `PLATE=... PLATE_HEAD=1 LEADIN=<딜 마지막 프레임> process-dealer-clip.sh sophia-reach-hr.webm ... reverse 0 26 56.56`.
- 결과: 딜·되돌리기의 모든 프레임에서 얼굴 밝기 98·채도 0.49·선명도 약 505로 일정(대기와 같다). 이음새 1.1~1.4.

## 카드를 내려놓는 되돌리기 (2026-09-25)

사용자 지적: 카드를 놓고 손이 돌아갈 때, 손이 테이블에 닿자마자 역재생처럼 튕겨 돌아간다.
- 원인: 되돌리기 영상은 손을 뻗는 영상(`reach`)을 거꾸로 튼 것이라 카드 위에 손을 얹는 순간이 없었다. 처음 8프레임 동안 딜 영상의 마지막 장면에서 크로스페이드해 팔이 두 겹으로 보였고, 곧바로 손이 되감기듯 움직였다.
- 새 영상은 두 생성 영상을 잇는다.
  - 앞 4프레임은 되돌리기 생성 영상(`sophia-return-hr2.webm`, 딜 원본 30프레임에서 시작) 0~3프레임이다. 카드 위에 손을 얹은 채 내려놓는 동작이다.
  - 3프레임 동안 손을 뻗는 영상(`sophia-reach-hr.webm`) 28프레임으로 섞는다. 두 영상 모두 손이 멈춰 있는 장면이라 겹침이 거의 없다.
  - 그 뒤 손을 뻗는 영상 28→3프레임을 거꾸로 쓴다. 손을 들어 펠트 위로 낮게 슈까지 가져가고, 속도는 천천히 시작해 천천히 멈춘다.
- 원본 조립:
  ```text
  ffmpeg -i sophia-return-hr2.webm -i sophia-reach-hr.webm -filter_complex "[0:v]trim=end_frame=4,setpts=N/24/TB,fps=24,settb=AVTB,format=yuv444p[a];[1:v]trim=start_frame=3:end_frame=29,reverse,setpts=N/24/TB,fps=24,settb=AVTB,format=yuv444p[b];[a][b]xfade=transition=fade:duration=0.125:offset=0.041667[out]" -map "[out]" -c:v ffv1 place-return-src3.mkv
  ```
- 후처리:
  ```text
  TONE=<기존 영상과 같은 톤 커브> PLATE=anchor-hr.png PLATE_HEAD=1 LEADIN=<딜 영상 마지막 프레임> LEADIN_FRAMES=2 LEADOUT=<대기 영상 첫 프레임> LEADOUT_FRAMES=3 process-dealer-clip.sh place-return-src3.mkv sophia-return once 0 27 56.56
  ```
  - `LEADOUT`(새 옵션): 마지막 프레임들을 대기 장면으로 섞어 끝 장면이 대기 영상의 첫 장면과 같게 한다.
- 결과: 27프레임, 1.125초. 모든 프레임에서 얼굴 밝기 98·채도 0.50·선명도 약 499로 일정하다. 이음새(평균 휘도 차)는 딜 끝→되돌리기 1.66, 되돌리기 끝→대기 1.42다. 웹은 `RETURN_CLIP_MS=1125`, `RETURN_REST_MS=1083`을 쓴다. 카드 사이에서는 약 2.5배속으로 튼다.
- 참고: 앞 영상(`sophia-return-hr2`)을 끝까지 쓰면 팔이 어깨 높이로 크게 돌아가고, 끝 자세도 대기 자세와 달라 쓰지 않았다.
