import { memo, useEffect, useMemo, useRef, useState } from "react";
import "./dealer-backdrop.css";
import { chooseDealerLayer, DEALER_LAYERS, layerClip, type ClipFlags, type DealerLayer, type DealerMood, type LayerFlags } from "../dealer-media";
import { type DealClip } from "../schedule";
import { liveServerNow } from "../clock";

export interface DealerBackdropProps {
  id: string;
  name: string;
  mood: DealerMood;
  /** 슈를 떠난 카드 수. 딜 중에 늘어나면 딜러가 다음 카드를 집는 동작을 이어 간다. */
  deal: DealClip | null;
  /** 아직 뒤집을 카드가 남아 있는지. 쇼다운에서 뒤집는 동작이 끝나도 카드가 남았으면 다시 한다. */
  flipsAhead?: boolean;
  poster: string;
  motionEnabled?: boolean;
}

const MOODS: DealerMood[] = ["idle", "deal", "flip"];
declare const __DEALER_CLIPS__: string[];
const availableClips = new Set(__DEALER_CLIPS__);
const hasClip = (id: string, mood: DealerMood) =>
  availableClips.has(`${id}-${mood}.webm`) || availableClips.has(`${id}-${mood}.mp4`);
export const hasDealerVideo = (id: string) => MOODS.some((mood) => hasClip(id, mood));

const mediaUrl = (id: string, mood: DealerMood, extension: "webm" | "mp4") =>
  `${import.meta.env.BASE_URL}dealers/${id}-${mood}.${extension}`;

/** 새 영상이 겹쳐 떠오르는 동안 이전 영상도 계속 움직인다 (CSS의 페이드 260ms보다 조금 길게). */
const OUTGOING_MS = 280;
/** 영상에서 사진으로 돌아갈 때 영상이 사라지는 시간 (CSS와 같게). */
const LEAVING_MS = 420;
/** 탭을 숨기거나 연출을 끈 동안 멈춘 동작은 이보다 짧게 멈췄을 때만 이어서 튼다 (길면 끝난 것으로 본다). */
const RESUME_LIMIT_MS = 1000;

const clockNow = () => (typeof performance !== "undefined" ? performance.now() : Date.now());

const prefersReducedMotion = () =>
  typeof window !== "undefined" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;

interface View {
  layer: DealerLayer | null;
  /** 바로 전에 보이던 층 (겹쳐 바꾸는 동안 계속 재생한다). */
  from: DealerLayer | null;
  restart: boolean;
  run: number;
  dealKey?: string;
}

function DealerBackdropInner({ id, name, mood, deal, flipsAhead = false, poster, motionEnabled }: DealerBackdropProps) {
  const layers = useMemo(() => DEALER_LAYERS.filter((layer) => hasClip(id, layerClip(layer))), [id]);
  const videos = useRef<Partial<Record<DealerLayer, HTMLVideoElement>>>({});
  // 층마다 한 번 만든 ref 콜백: 다시 그릴 때마다 붙였다 떼지 않는다.
  const refs = useMemo(() => {
    const out = {} as Record<DealerLayer, (video: HTMLVideoElement | null) => void>;
    for (const layer of DEALER_LAYERS) {
      out[layer] = (video) => {
        if (video) videos.current[layer] = video;
        else delete videos.current[layer];
      };
    }
    return out;
  }, []);
  const [ready, setReady] = useState<LayerFlags>({});
  const [failed, setFailed] = useState<ClipFlags>({});
  const [endedTick, setEndedTick] = useState(0);
  const [view, setView] = useState<View>({ layer: null, from: null, restart: false, run: 0 });
  const viewRef = useRef(view);
  viewRef.current = view;
  const pauseTimers = useRef<Partial<Record<DealerLayer, number>>>({});
  const [documentVisible, setDocumentVisible] = useState(() =>
    typeof document === "undefined" || document.visibilityState === "visible",
  );
  const [reducedMotion, setReducedMotion] = useState(prefersReducedMotion);
  const [posterFailed, setPosterFailed] = useState(false);
  const motionAllowed = motionEnabled ?? !reducedMotion;
  /** 영상을 틀 수 있는지 (탭이 보이고 연출이 켜져 있다). */
  const awake = motionAllowed && documentVisible;
  const wasAwake = useRef(awake);
  /** 마지막으로 잠든(탭 숨김·연출 끔) 시각. 처음부터 잠들어 있었으면 null (얼마나 멈췄는지 모른다). */
  const sleptAt = useRef<number | null>(null);
  /** 처음부터 다시 틀기(restart)를 이미 적용한 실행 번호. 한 실행에 한 번만 되감는다. */
  const restartApplied = useRef(-1);
  /** 깨어날 때 끝난 것으로 본 실행 번호: 다시 고를 때까지 틀지 않고 멈춘 자리에 둔다. */
  const staleRun = useRef(-1);

  useEffect(() => {
    const media = window.matchMedia("(prefers-reduced-motion: reduce)");
    const syncMotion = () => setReducedMotion(media.matches);
    const syncVisibility = () => setDocumentVisible(document.visibilityState === "visible");
    syncMotion();
    syncVisibility();
    media.addEventListener("change", syncMotion);
    document.addEventListener("visibilitychange", syncVisibility);
    const timers = pauseTimers.current;
    return () => {
      media.removeEventListener("change", syncMotion);
      document.removeEventListener("visibilitychange", syncVisibility);
      for (const timer of Object.values(timers)) window.clearTimeout(timer);
    };
  }, []);

  const usable = useMemo(() => {
    const out: LayerFlags = {};
    for (const layer of layers) out[layer] = !!ready[layer] && !failed[layerClip(layer)];
    return out;
  }, [layers, ready, failed]);

  // 보일 층을 정한다: 동작이 바뀔 때, 카드가 슈를 떠날 때, 동작 영상이 끝날 때, 탭·연출이 돌아올 때.
  useEffect(() => {
    const woke = awake && !wasAwake.current;
    if (!awake && wasAwake.current) sleptAt.current = clockNow();
    wasAwake.current = awake;
    const current = viewRef.current;
    const video = current.layer ? videos.current[current.layer] : undefined;
    const duration = video?.duration ?? 0;
    const progress = video && Number.isFinite(duration) && duration > 0 ? video.currentTime / duration : 0;
    let ended = !!video?.ended;
    if (woke && video && current.layer !== null && current.layer !== "idle") {
      // 숨긴 동안 멈춰 둔 동작: 아직 틀지 못했거나(되감긴 채) 오래 멈췄으면 끝난 것으로 본다.
      // 그대로 이어 틀면 카드 없이 동작만 처음부터 다시 하게 된다.
      const slept = sleptAt.current === null ? Infinity : clockNow() - sleptAt.current;
      const unplayed = (current.restart && restartApplied.current !== current.run) || video.currentTime === 0;
      if (unplayed || slept > RESUME_LIMIT_MS) {
        ended = true;
        staleRun.current = current.run;
      }
    }
    if (mood === "deal") return;
    const choice = chooseDealerLayer(mood, {
      current: current.layer,
      progress,
      ended,
      newCard: false,
      flipsAhead,
      usable,
    });
    if (choice.layer === current.layer && !choice.restart) return;
    setView({ layer: choice.layer, from: current.layer, restart: choice.restart, run: current.run + 1 });
  }, [mood, flipsAhead, usable, endedTick, awake]);

  useEffect(() => {
    if (!deal || !awake) return;
    const current = viewRef.current;
    if (current.dealKey === deal.key && (current.layer === "deal" || current.layer === "deal2")) return;
    const layer: DealerLayer = current.layer === "deal" && usable.deal2 ? "deal2" : usable.deal ? "deal" : "deal2";
    setView({ layer, from: current.layer, restart: true, run: current.run + 1, dealKey: deal.key });
  }, [deal, awake, usable]);

  // 재생: 보이는 층만 틀고, 바로 전 층은 겹쳐 바뀌는 동안 조금 더 움직이다 멈춘다.
  // 멈춘 동작 영상은 보이지 않을 때 처음으로 되감아 두어, 다음에 첫 프레임부터 바로 보인다.
  // 탭을 숨기거나 연출을 꺼서 멈추는 지금 층은 되감지 않는다 (돌아왔을 때 이어 틀거나 끝난 것으로 본다).
  useEffect(() => {
    const active = awake && staleRun.current !== view.run ? view.layer : null;
    for (const layer of layers) {
      const video = videos.current[layer];
      if (!video) continue;
      const timers = pauseTimers.current;
      if (timers[layer] !== undefined) {
        window.clearTimeout(timers[layer]);
        delete timers[layer];
      }
      if (layer === active) {
        // 처음부터 다시 틀기는 이 실행에서 한 번만: 숨겼다 돌아와 이 효과가 다시 돌아도 되감지 않는다.
        if (view.restart && restartApplied.current !== view.run && layer !== "idle") {
          if (view.dealKey && deal?.key === view.dealKey) {
            video.playbackRate = deal.rate;
            video.currentTime = Math.min(0.867, Math.max(0, (liveServerNow() - deal.startAt) * deal.rate / 1000));
          } else if (video.currentTime > 0) {
            video.currentTime = 0;
            video.playbackRate = 1;
          }
        }
        restartApplied.current = view.run;
        void video.play().catch(() => {
          // 자동 재생이 막혀도 영상 파일 문제는 아니다. 준비된 프레임을 그대로 둔다.
        });
        continue;
      }
      if (layer === view.layer) {
        video.pause();
        continue;
      }
      // 끝나서 멈춘 동작 영상도 처음으로 되감아 둔다 (다음에 끝 프레임이 비치지 않게).
      if (video.paused && (layer === "idle" || video.currentTime === 0)) continue;
      const settle = () => {
        delete timers[layer];
        video.pause();
        if (layer !== "idle" && video.currentTime > 0) video.currentTime = 0;
      };
      if (layer === view.from && active !== null) timers[layer] = window.setTimeout(settle, OUTGOING_MS);
      else if (layer === view.from && awake) timers[layer] = window.setTimeout(settle, LEAVING_MS);
      else settle();
    }
  }, [view, awake, layers]);

  const markReady = (layer: DealerLayer) => setReady((current) => (current[layer] ? current : { ...current, [layer]: true }));
  const markFailed = (clip: DealerMood) => setFailed((current) => (current[clip] ? current : { ...current, [clip]: true }));
  const videoLive = motionAllowed && view.layer !== null;
  // 대기 영상이 없거나 못 쓰면 사진이 아주 천천히 숨 쉬듯 움직인다 (연출을 켰을 때만).
  const breathe = motionAllowed && (!layers.includes("idle") || !!failed.idle);

  return (
    <div
      className="dealer-backdrop-layer"
      data-reduced-motion={!motionAllowed ? "true" : "false"}
      data-video-active={videoLive ? "true" : "false"}
      data-breathe={breathe ? "true" : "false"}
      aria-label={`AI 딜러 ${name}`}
    >
      <img
        className="dealer-backdrop-poster"
        src={posterFailed ? `${import.meta.env.BASE_URL}dealer.png` : poster}
        alt={`테이블의 AI 딜러 ${name}`}
        onError={() => setPosterFailed(true)}
      />
      {layers.map((layer) => {
        const clip = layerClip(layer);
        const state = !motionAllowed
          ? ""
          : layer === view.layer
            ? "is-active"
            : layer === view.from && view.layer === null
              ? "is-leaving"
              : "";
        return (
          <video
            key={layer}
            ref={refs[layer]}
            className={`dealer-backdrop-media ${state}`}
            autoPlay={false}
            muted
            loop={layer === "idle"}
            playsInline
            preload="auto"
            aria-hidden="true"
            onCanPlay={() => markReady(layer)}
            onEnded={() => setEndedTick((tick) => tick + 1)}
            onError={() => markFailed(clip)}
            data-layer={layer}
          >
            {availableClips.has(`${id}-${clip}.webm`) && (
              <source
                src={mediaUrl(id, clip, "webm")}
                type="video/webm"
                onError={() => {
                  if (!availableClips.has(`${id}-${clip}.mp4`)) markFailed(clip);
                }}
              />
            )}
            {availableClips.has(`${id}-${clip}.mp4`) && <source src={mediaUrl(id, clip, "mp4")} type="video/mp4" onError={() => markFailed(clip)} />}
          </video>
        );
      })}
    </div>
  );
}

export const DealerBackdrop = memo(function DealerBackdrop(props: DealerBackdropProps) {
  return <DealerBackdropInner key={props.id} {...props} />;
});
