# 실사 딜러 에셋 (2026-09-23)

## 들어 있는 파일

| 파일 | 내용 |
|---|---|
| `sophia-table.png` | 원본 `../dealer.png`를 편집한 실사 정지 이미지(1536×1024). 팔에 연결된 손과 카드 슈를 더했다. 연출을 끄면 이 사진이 보인다. |
| `sophia-video.jpg` | 영상 첫 프레임(768×512). 연출을 켜면 영상이 뜨기 전까지 이 사진을 보여 줘 딜러 얼굴이 바뀌지 않게 한다. |
| `sophia-idle.webm` / `.mp4` | 대기 루프 약 4초. 2초 생성본을 앞으로 재생한 뒤 거꾸로 이어 끊김 없이 반복한다. |
| `sophia-deal.webm` / `.mp4` | 약 2초. 슈에서 카드를 꺼내 앞에 놓는다. |
| `sophia-flip.webm` / `.mp4` | 약 2초. 가운데 카드를 손으로 연다. |

동작 영상은 기본 자세(오른손은 슈 위)로 돌아오지 않고 끝난다. 웹은 영상 사이를 450ms 동안 겹쳐 바꿔(`dealer-backdrop.css`), 대기 영상으로 돌아갈 때 손 위치가 튀지 않고 스며들 듯 바뀐다.

WebM은 VP9(CRF 34), MP4는 H.264(CRF 25, faststart)다. 브라우저는 WebM을 먼저 쓰고 못 쓰면 MP4를 쓴다. Safari·iOS는 바이트 범위 요청으로만 영상을 재생하므로 서버가 206 응답을 준다(`src/casino_web.rs`).

## 만드는 방법 (외부 API 없이 이 PC에서)

모델이 원본 사진의 얼굴(768×512에서 폭 50px 정도)을 그대로 유지하지 못한다. 원본 사진에서 바로 만들면 첫 프레임 뒤에 얼굴과 슈가 다른 모습으로 바뀌고, 영상마다 다른 사람이 된다. 그래서 다음 순서로 만든다.

1. 원본 사진에서 딜링 영상을 한 번 만들고(원본 VAE), 카메라를 보고 손이 제자리에 있는 초반 프레임(6번째)을 **기준 프레임**으로 뽑는다. 모델이 이미 그린 얼굴이라 이후 영상에서 바뀌지 않는다.
2. 대기·딜링·플립을 모두 기준 프레임에서 시작해 만든다(49프레임, 2초). 세 영상의 얼굴과 슈가 같다.
3. 동작 끝에서 손을 제자리로 되돌리는 `return` 구간도 두 번 만들어 봤지만 버렸다. 첫 시도는 얼굴이 크게 웃는 모양으로 일그러지고 팔이 비정상적으로 늘어났으며 마지막 프레임에 흰 사각형이 생겼다. 두 번째는 흰 조각이 흩어지고 두 팔을 넓게 벌린 채 끝났다. 대신 웹의 교차 전환으로 이음새를 가린다. `-Mood return`은 다시 시도할 때를 위해 남겨 둔다.
4. 빠른 TAE 디코더로 만든 프레임을 다시 기준으로 쓰면 대비가 누적돼 팔이 하얗게 날아간다. 기준 프레임과 최종본은 원본 VAE로만 만든다.

```powershell
# casino-web 디렉터리 기준. RuntimeRoot 구성은 아래 절 참고.
./tools/generate-dealer-local.ps1 -RuntimeRoot C:/temp/casino-local-video -Mood deal -Frames 49 -Reference C:/temp/casino-local-video/anchor/anchor-b.png -Suffix -b
./tools/generate-dealer-local.ps1 -RuntimeRoot C:/temp/casino-local-video -Mood idle -Frames 49 -Reference C:/temp/casino-local-video/anchor/anchor-b.png -Suffix -b
./tools/generate-dealer-local.ps1 -RuntimeRoot C:/temp/casino-local-video -Mood flip -Frames 49 -Reference C:/temp/casino-local-video/anchor/anchor-b.png -Suffix -b
```

기준 프레임 `anchor-b.png`는 원본 사진에서 만든 원본 VAE 딜링 영상(`sophia-deal-hq.webm`)의 6번째 프레임이다. 생성 설정: 768×512, 24fps, seed 473, euler 샘플러 + LCM 스케줄러 3스텝(`-Sampling dmd`는 DMD 시그마 `1,0.757,0.522,0`), CFG 1, 원본 Wan 2.2 VAE 8×8 타일, VRAM 예산 3GB. 49프레임 한 개에 약 15분(샘플링 4분 + VAE 디코딩 11분). 스크립트는 사진을 ASCII 경로로 복사해 쓰고, 기존 결과는 덮어쓰지 않으며, 설정 JSON의 `reviewed`는 기본 false다. 조립(루프·이어 붙이기·인코딩)은 `C:/temp/casino-local-video/assemble.sh`에 있다.

## 검수 결과

- 채택: 기준 프레임에서 만든 대기·딜링·플립(각 2초). 세 영상의 얼굴·옷·슈가 같고 번짐이나 손 분리가 없다.
- 한계: 원본 사진보다 부드럽다. 딜링 중 움직이는 카드는 흰 사각형으로 번져 보이고, 플립 동작은 작다. 원본 사진의 딜러와 얼굴이 조금 다르므로 연출을 켜면 영상 첫 프레임을 포스터로 쓴다. 사이드 패널 아바타도 연출이 켜져 있으면 영상 포스터 얼굴을 쓴다. Discord 웹훅 아바타는 원본 사진(`../dealer.png`) 얼굴이다.
- 버림: 원본 사진에서 바로 만든 대기·딜링·플립(첫 프레임 뒤 얼굴과 슈가 바뀜, 플립은 손이 분홍색으로 뭉개짐), TAE로 디코딩한 기준 프레임 체인(대비 누적), 576×384 초안, 복귀 구간 두 번.

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

## 사용한 이미지 편집 프롬프트 (`sophia-table.png`)

Use case: identity-preserve / precise-object-edit. Edit the supplied casino dealer photograph for a production web blackjack and Texas Hold'em table. Preserve exactly the fictional adult woman's facial identity, black sleeveless qipao with fine gold piping, hairstyle, camera viewpoint, elegant warm brass casino room, emerald felt and lighting. Create ONE photoreal photograph, same 1536x1024 landscape composition. Change only her arms/hands and add a real casino card shoe: her anatomically correct right hand with five natural fingers is resting lightly at the opening of a realistic polished black-and-dark-walnut 8-deck casino card shoe with transparent acrylic ramp, visible stacked card edges and subtle brass fittings at image right (about x80%, y64%); her left hand rests naturally flat on the felt near the center, ready to slide a face-down card. Hands MUST be physically connected to her forearms, skin texture and perspective consistent, no floating props. Keep most of the lower 35% felt clear for interactive playing cards and player seats. The shoe should be compact, real scale, with believable contact shadows/reflections, not a giant box. She is looking calmly at camera. No drawn hands, no 2D illustration, no cartoon, no CGI plastic look, no text, no UI, no logos. This is a still reference/poster for a future actual dealer motion video; do not render a storyboard or multiple frames.
