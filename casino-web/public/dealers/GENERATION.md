# 실사 딜러 에셋 (2026-09-23)

## 현재 생성된 파일

- `sophia-table.png`: 내장 image_gen 도구로 원본 `../dealer.png`를 편집한 실사 정지 이미지(1536×1024). 얼굴·의상·조명 유지, 팔에 연결된 손과 실제 카드 슈 추가.
- 영상 생성 도구/API가 연결되지 않아 **동영상 파일은 아직 없다**. PNG 이동이나 프레임 전환을 실제 딜링 영상이라고 처리하지 않는다.
- 원본 `dealer.png`는 유지한다. 사진 생성 모드: built-in edit, identity-preserve / precise-object-edit.

## 사용한 이미지 생성 프롬프트

Use case: identity-preserve / precise-object-edit. Edit the supplied casino dealer photograph for a production web blackjack and Texas Hold'em table. Preserve exactly the fictional adult woman's facial identity, black sleeveless qipao with fine gold piping, hairstyle, camera viewpoint, elegant warm brass casino room, emerald felt and lighting. Create ONE photoreal photograph, same 1536x1024 landscape composition. Change only her arms/hands and add a real casino card shoe: her anatomically correct right hand with five natural fingers is resting lightly at the opening of a realistic polished black-and-dark-walnut 8-deck casino card shoe with transparent acrylic ramp, visible stacked card edges and subtle brass fittings at image right (about x80%, y64%); her left hand rests naturally flat on the felt near the center, ready to slide a face-down card. Hands MUST be physically connected to her forearms, skin texture and perspective consistent, no floating props. Keep most of the lower 35% felt clear for interactive playing cards and player seats. The shoe should be compact, real scale, with believable contact shadows/reflections, not a giant box. She is looking calmly at camera. No drawn hands, no 2D illustration, no cartoon, no CGI plastic look, no text, no UI, no logos. This is a still reference/poster for a future actual dealer motion video; do not render a storyboard or multiple frames.

## 영상 도구 연결 후 사용할 생성 지시

입력: `sophia-table.png`. 카메라 고정, 인물 동일성·얼굴·의상·카지노·조명·슈 위치 유지. 손가락 개수와 카드 형상을 프레임마다 유지하고 팔·손을 분리하지 않는다. 자막·브랜드·음성 없음. 첫/끝 프레임이 자연스럽게 이어지는 루프, 24~30fps, 원본과 같은 3:2 구도.

- `sophia-idle.webm` 또는 `.mp4`: 6~8초. 자연스러운 호흡·눈 깜박임, 손은 슈와 펠트 위에 유지.
- `sophia-deal.webm` 또는 `.mp4`: 3~4초. 슈 입구에서 카드 한 장을 꺼내 테이블 앞쪽으로 밀고 손을 원위치로 회수. 카드는 화면 하단의 실제 UI 카드 영역 직전까지만 이동. 카메라 이동·줌 금지.
- `sophia-flip.webm` 또는 `.mp4`: 2~3초. 중앙의 뒷면 카드를 손가락으로 집어 뒤집고 손을 원위치로 회수. 카드 면의 숫자·기호는 UI 결과와 충돌하지 않도록 읽히지 않게 배치.

생성 후 영상의 손·슈·카드 연속성을 직접 확인하고 같은 폴더에 넣어 빌드한다. 기존 사진만으로 실제 영상이 자동 생성되지는 않는다.
