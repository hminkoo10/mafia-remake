// 딜러 영상 층 고르기 (순수 함수, node 테스트에서 바로 불러 쓴다).
export type DealerMood = "idle" | "deal" | "flip";
/**
 * 영상 층. 딜 동작은 두 층(deal, deal2)을 번갈아 써서 다음 카드 동작을 겹쳐 이어 간다.
 * 뒤집는 동작도 두 층(flip, flip2)을 번갈아 써서, 쇼다운에서 뒤집을 카드가 남아 있으면 다음 동작으로 이어 간다.
 */
export type DealerLayer = "idle" | "deal" | "deal2" | "flip" | "flip2";
export type ClipFlags = Partial<Record<DealerMood, boolean>>;
export type LayerFlags = Partial<Record<DealerLayer, boolean>>;

export const DEALER_LAYERS: readonly DealerLayer[] = ["idle", "deal", "deal2", "flip", "flip2"];
/** 딜 동작이 이만큼 진행됐으면 다음 카드 때 새 동작으로 이어 간다. */
export const DEAL_RESTART_PROGRESS = 0.7;

export const layerClip = (layer: DealerLayer): DealerMood => (layer === "deal2" ? "deal" : layer === "flip2" ? "flip" : layer);

export interface DealerLayerState {
  /** 지금 보이는 층. null이면 영상 없이 사진(포스터). */
  current: DealerLayer | null;
  /** 보이는 영상의 진행률 (0~1). */
  progress: number;
  /** 보이는 동작 영상이 끝났는지 (동작 영상은 반복하지 않는다). */
  ended: boolean;
  /** 이번에 새 카드가 슈를 떠났는지. */
  newCard: boolean;
  /** 아직 뒤집을 카드가 남아 있는지 (쇼다운에서 여러 사람의 카드를 차례로 뒤집는 중). */
  flipsAhead?: boolean;
  /** 재생할 수 있는 층 (불러왔고 실패하지 않음). */
  usable: LayerFlags;
}

export interface DealerLayerChoice {
  layer: DealerLayer | null;
  /** 동작 영상을 처음부터 다시 튼다. */
  restart: boolean;
}

const isGesture = (layer: DealerLayer | null): layer is DealerLayer => layer !== null && layer !== "idle";
const isFlip = (layer: DealerLayer | null): layer is "flip" | "flip2" => layer === "flip" || layer === "flip2";

/**
 * 보일 영상 층을 고른다.
 * - deal: 카드가 남아 있는 동안 손이 멈추지 않게, 동작이 끝났거나 70% 넘게 진행된 뒤 새 카드가 나가면
 *   다른 딜 층에서 처음부터 다시 시작한다 (느리게 틀어 늘리지 않는다).
 * - flip: 뒤집는 동작을 한 번 한다. 동작이 끝났는데 뒤집을 카드가 아직 남아 있으면(여러 사람의 쇼다운)
 *   다른 뒤집기 층에서 처음부터 다시 한다.
 * - idle: 하던 동작은 끝까지 하고, 끝난 동작은 멈춘 채 두지 않고 대기 영상(없으면 사진)으로 돌아간다.
 * 요청한 영상을 쓸 수 없으면 없는 동작을 지어내지 않는다.
 */
export function chooseDealerLayer(requested: DealerMood, state: DealerLayerState): DealerLayerChoice {
  const { current, usable } = state;
  const live = current !== null && !!usable[current] && !state.ended;
  const keep: DealerLayerChoice = { layer: current, restart: false };
  const start = (layer: DealerLayer): DealerLayerChoice => ({ layer, restart: true });

  if (requested === "deal") {
    if (current === "deal" || current === "deal2") {
      if (live && !(state.newCard && state.progress >= DEAL_RESTART_PROGRESS)) return keep;
      const other: DealerLayer = current === "deal" ? "deal2" : "deal";
      if (usable[other]) return start(other);
      if (usable[current]) return start(current);
    } else if (usable.deal) {
      return start("deal");
    } else if (usable.deal2) {
      return start("deal2");
    }
  } else if (requested === "flip") {
    if (isFlip(current)) {
      if (live) return keep;
      if (state.flipsAhead) {
        const other: DealerLayer = current === "flip" ? "flip2" : "flip";
        if (usable[other]) return start(other);
        if (usable[current]) return start(current);
      }
    } else if (usable.flip) {
      return start("flip");
    } else if (usable.flip2) {
      return start("flip2");
    }
  }
  // 대기로 돌아가거나 요청한 영상을 못 쓰면: 하던 동작은 끝까지, 끝났으면 대기 영상 → 사진.
  if (isGesture(current) && live) return keep;
  if (usable.idle) return { layer: "idle", restart: false };
  return { layer: null, restart: false };
}
