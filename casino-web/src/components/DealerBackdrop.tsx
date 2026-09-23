import { memo, useEffect, useRef, useState } from "react";
import "./dealer-backdrop.css";
import { selectDealerClip, shouldFinishDealerGesture, type DealerMood } from "../dealer-media";

export interface DealerBackdropProps {
  id: string;
  name: string;
  mood: DealerMood;
  poster: string;
  motionEnabled?: boolean;
}

const MOODS: DealerMood[] = ["idle", "deal", "flip"];
declare const __DEALER_CLIPS__: string[];
const availableClips = new Set(__DEALER_CLIPS__);
export const hasDealerVideo = (id: string) => MOODS.some((mood) =>
  availableClips.has(`${id}-${mood}.webm`) || availableClips.has(`${id}-${mood}.mp4`),
);

const mediaUrl = (id: string, mood: DealerMood, extension: "webm" | "mp4") =>
  `${import.meta.env.BASE_URL}dealers/${id}-${mood}.${extension}`;

const prefersReducedMotion = () =>
  typeof window !== "undefined" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;

function DealerBackdropInner({ id, name, mood, poster, motionEnabled }: DealerBackdropProps) {
  const videos = useRef<Partial<Record<DealerMood, HTMLVideoElement>>>({});
  const startedMood = useRef<DealerMood | null>(null);
  const [visibleMood, setVisibleMood] = useState<DealerMood | null>(null);
  const [ready, setReady] = useState<Partial<Record<DealerMood, boolean>>>({});
  const [failed, setFailed] = useState<Partial<Record<DealerMood, boolean>>>({});
  const [ended, setEnded] = useState<Partial<Record<DealerMood, boolean>>>({});
  const [documentVisible, setDocumentVisible] = useState(() =>
    typeof document === "undefined" || document.visibilityState === "visible",
  );
  const [reducedMotion, setReducedMotion] = useState(prefersReducedMotion);
  const [posterFailed, setPosterFailed] = useState(false);
  const motionAllowed = motionEnabled ?? !reducedMotion;

  useEffect(() => {
    let disposed = false;
    const media = window.matchMedia("(prefers-reduced-motion: reduce)");
    const syncMotion = () => setReducedMotion(media.matches);
    const syncVisibility = () => setDocumentVisible(document.visibilityState === "visible");

    syncMotion();
    syncVisibility();
    media.addEventListener("change", syncMotion);
    document.addEventListener("visibilitychange", syncVisibility);

    // Start all three resource loads once. The elements stay mounted so mood changes do not refetch.
    const preload = window.setTimeout(() => {
      if (disposed) return;
      for (const video of Object.values(videos.current)) video?.load();
    }, 0);

    return () => {
      disposed = true;
      window.clearTimeout(preload);
      media.removeEventListener("change", syncMotion);
      document.removeEventListener("visibilitychange", syncVisibility);
    };
  }, []);

  useEffect(() => {
    setVisibleMood((current) => {
      const video = current ? videos.current[current] : null;
      const playing = Boolean(motionAllowed && video && !video.paused && !video.ended && current && !failed[current] && !ended[current]);
      if (shouldFinishDealerGesture(mood, current, playing)) return current;
      return selectDealerClip(mood, current, ready, failed);
    });
  }, [mood, ready, failed, ended, motionAllowed]);

  useEffect(() => {
    for (const video of Object.values(videos.current)) {
      if (!video) continue;
      const shouldPlay =
        documentVisible && motionAllowed && visibleMood !== null && video === videos.current[visibleMood];

      if (!shouldPlay) {
        video.pause();
        continue;
      }

      const shouldRestart = visibleMood === mood && (mood === "deal" || mood === "flip") && startedMood.current !== mood;
      if (shouldRestart) {
        video.currentTime = 0;
        setEnded((current) => current[mood] ? { ...current, [mood]: false } : current);
      }
      // Resuming a tab or toggling motion must not replay an already completed gesture.
      if (video.ended && !shouldRestart) continue;
      void video.play().catch(() => {
        // Autoplay rejection is not a missing-media failure; leave the ready frame visible.
      });
    }
    // idle을 거쳐 다시 같은 동작을 요청하면 새 동작으로 시작한다.
    startedMood.current = visibleMood;
  }, [documentVisible, mood, motionAllowed, visibleMood]);

  const markReady = (clipMood: DealerMood) => {
    setReady((current) => (current[clipMood] ? current : { ...current, [clipMood]: true }));
  };

  const markFailed = (clipMood: DealerMood) => {
    setFailed((current) => (current[clipMood] ? current : { ...current, [clipMood]: true }));
  };

  return (
    <div
      className="dealer-backdrop-layer"
      data-reduced-motion={!motionAllowed ? "true" : "false"}
      data-video-active={visibleMood !== null && motionAllowed ? "true" : "false"}
      aria-label={`AI 딜러 ${name}`}
    >
      <img className="dealer-backdrop-poster" src={posterFailed ? `${import.meta.env.BASE_URL}dealer.png` : poster} alt={`테이블의 AI 딜러 ${name}`} onError={() => setPosterFailed(true)} />
      {MOODS.filter((clipMood) => availableClips.has(`${id}-${clipMood}.webm`) || availableClips.has(`${id}-${clipMood}.mp4`)).map((clipMood) => (
        <video
          key={clipMood}
          ref={(video) => {
            if (video) videos.current[clipMood] = video;
            else delete videos.current[clipMood];
          }}
          className={`dealer-backdrop-media ${visibleMood === clipMood && motionAllowed ? "is-active" : ""}`}
          autoPlay={false}
          muted
          loop={clipMood === "idle"}
          playsInline
          preload="auto"
          aria-hidden="true"
          onCanPlay={() => markReady(clipMood)}
          onEnded={() => setEnded((current) => ({ ...current, [clipMood]: true }))}
          onError={() => markFailed(clipMood)}
          data-ready={ready[clipMood] ? "true" : "false"}
          data-failed={failed[clipMood] ? "true" : "false"}
        >
          {availableClips.has(`${id}-${clipMood}.webm`) && <source src={mediaUrl(id, clipMood, "webm")} type="video/webm" onError={() => { if (!availableClips.has(`${id}-${clipMood}.mp4`)) markFailed(clipMood); }} />}
          {availableClips.has(`${id}-${clipMood}.mp4`) && <source src={mediaUrl(id, clipMood, "mp4")} type="video/mp4" onError={() => markFailed(clipMood)} />}
        </video>
      ))}
    </div>
  );
}

export const DealerBackdrop = memo(function DealerBackdrop(props: DealerBackdropProps) {
  return <DealerBackdropInner key={props.id} {...props} />;
});
