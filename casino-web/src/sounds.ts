// 효과음: 외부 파일 없이 Web Audio로 합성한다 (카드 딜, 칩, 베팅 확정, 내 차례, 승/패).
// 브라우저 정책상 첫 사용자 입력 뒤에만 소리가 난다.

import { MUSIC_BPM, MUSIC_CYCLE_BEATS, MUSIC_LOOK_AHEAD_SECONDS, MUSIC_MASTER_GAIN, musicNotesForCycle, notesInWindow, type MusicNote } from "./music";

let context: AudioContext | null = null;
let noise: AudioBuffer | null = null;
let musicMaster: GainNode | null = null;
let musicTimer: number | null = null;
let musicCycle = 0;
let musicNextBeat = 0;
let musicHidden = false;
let musicPreference = readMusicPreference();

function readMusicPreference(): boolean {
  try { return localStorage.getItem("noir-music") !== "0"; } catch { return true; }
}

/**
 * 오디오를 준비한다. AudioContext를 처음 만들 때 기기에 따라 수백 ms 동안 화면이 멈추므로,
 * 카드를 나눠 주는 도중이 아니라 사용자가 처음 누른 직후(`installAudioUnlock`)에 만든다.
 */
export function unlockAudio() {
  if (context) {
    if (context.state === "suspended") void context.resume();
    return;
  }
  const Ctor = window.AudioContext || (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
  if (!Ctor) return;
  try {
    context = new Ctor();
  } catch {
    context = null;
  }
}

/** 첫 클릭·키 입력·터치 때 한 번 오디오를 준비한다. 되돌리는 함수를 준다. */
export function installAudioUnlock(onUnlock?: () => void): () => void {
  const events = ["pointerdown", "keydown", "touchstart"] as const;
  const unlock = () => {
    remove();
    // 누른 동작의 화면 반응을 먼저 그리고 나서 만든다.
    window.setTimeout(() => { unlockAudio(); onUnlock?.(); }, 0);
  };
  const remove = () => events.forEach((name) => window.removeEventListener(name, unlock, true));
  events.forEach((name) => window.addEventListener(name, unlock, { capture: true, passive: true }));
  return remove;
}

function audio(): AudioContext | null {
  // 아직 준비 전이면 이번 소리는 건너뛴다 (여기서 만들면 딜 도중에 화면이 멈춘다).
  if (!context) return null;
  if (context.state === "suspended") void context.resume();
  return context;
}

/** 카드 스치는 소리에 쓰는 잡음. 한 번만 만들어 둔다. */
function noiseBuffer(ctx: AudioContext): AudioBuffer {
  if (noise && noise.sampleRate === ctx.sampleRate) return noise;
  const length = Math.floor(ctx.sampleRate * 0.25);
  const buffer = ctx.createBuffer(1, length, ctx.sampleRate);
  const data = buffer.getChannelData(0);
  for (let i = 0; i < length; i++) data[i] = Math.random() * 2 - 1;
  noise = buffer;
  return buffer;
}

export function soundEnabled(): boolean {
  try {
    return localStorage.getItem("noir-sfx") !== "0";
  } catch {
    return true;
  }
}

export function setSoundEnabled(enabled: boolean) {
  try {
    localStorage.setItem("noir-sfx", enabled ? "1" : "0");
  } catch {
    // 저장이 막혀 있어도 이번 세션에서는 동작한다.
  }
  updateMusicGain();
}

export function musicEnabled(): boolean { return musicPreference; }
export function setMusicEnabled(enabled: boolean) { musicPreference = enabled; try { localStorage.setItem("noir-music", enabled ? "1" : "0"); } catch {} syncMusic(); }
function updateMusicGain() { if (!musicMaster || !context) return; const target = musicPreference && soundEnabled() ? MUSIC_MASTER_GAIN : 0; const now = context.currentTime; musicMaster.gain.cancelScheduledValues(now); musicMaster.gain.setTargetAtTime(target, now, 0.04); }
/** 배경음악 음 하나. 음마다 짧게 살다 멈추는 노드를 만든다 (다른 곳에서 쥐고 있지 않는다). */
function musicNote(ctx: AudioContext, note: MusicNote, start: number) {
  const master = musicMaster;
  if (!master) return;
  const gain = ctx.createGain();
  gain.gain.setValueAtTime(0.0001, start);
  gain.gain.exponentialRampToValueAtTime(note.gain, start + (note.kind === "bass" ? 0.025 : 0.08));
  gain.gain.exponentialRampToValueAtTime(0.0001, start + note.duration);
  gain.connect(master);
  if (note.kind === "shaker") {
    const source = ctx.createBufferSource();
    const filter = ctx.createBiquadFilter();
    source.buffer = noiseBuffer(ctx);
    filter.type = "highpass";
    filter.frequency.setValueAtTime(3600, start);
    source.connect(filter).connect(gain);
    source.start(start, 0, Math.min(note.duration, 0.25));
    source.stop(start + note.duration + 0.02);
    return;
  }
  const frequency = note.frequency ?? 220;
  const osc = ctx.createOscillator();
  osc.type = note.kind === "vibe" ? "triangle" : "sine";
  osc.frequency.setValueAtTime(frequency, start);
  osc.connect(gain);
  osc.start(start);
  osc.stop(start + note.duration + 0.03);
  if (note.kind === "chord" || note.kind === "vibe") {
    // 전기 피아노·비브라폰처럼 치는 순간에만 밝은 배음이 났다가 빨리 사라진다 (사인파만이면 오르간처럼 밋밋하다).
    const bell = ctx.createOscillator();
    const bellGain = ctx.createGain();
    bell.type = "sine";
    bell.frequency.setValueAtTime(frequency * (note.kind === "vibe" ? 4 : 2), start);
    bellGain.gain.setValueAtTime(0.0001, start);
    bellGain.gain.exponentialRampToValueAtTime(note.gain * 0.35, start + 0.012);
    bellGain.gain.exponentialRampToValueAtTime(0.0001, start + 0.55);
    bell.connect(bellGain).connect(master);
    bell.start(start);
    bell.stop(start + 0.6);
  }
}
function scheduleMusic() { musicTimer = null; if (musicHidden || !musicPreference || !soundEnabled()) return; const ctx = audio(); if (!ctx || !musicMaster) return; const spb = 60 / MUSIC_BPM; const horizon = ctx.currentTime + MUSIC_LOOK_AHEAD_SECONDS; while (musicNextBeat * spb < horizon) { const cycleStart = musicCycle * MUSIC_CYCLE_BEATS; const from = musicNextBeat - cycleStart; const to = from + MUSIC_LOOK_AHEAD_SECONDS / spb; for (const note of notesInWindow(musicNotesForCycle(musicCycle), from, to)) musicNote(ctx, note, (cycleStart + note.beat) * spb + 0.02); musicNextBeat += MUSIC_LOOK_AHEAD_SECONDS / spb; if (musicNextBeat >= (musicCycle + 1) * MUSIC_CYCLE_BEATS) musicCycle += 1; } musicTimer = window.setTimeout(scheduleMusic, 100); }
export function startMusic() { const ctx = audio(); if (!ctx || !musicPreference || !soundEnabled() || musicHidden) return; if (!musicMaster) { musicMaster = ctx.createGain(); const filter = ctx.createBiquadFilter(); filter.type = "lowpass"; filter.frequency.setValueAtTime(2200, ctx.currentTime); musicMaster.connect(filter).connect(ctx.destination); } if (musicTimer === null) { musicNextBeat = ctx.currentTime / (60 / MUSIC_BPM); musicCycle = Math.floor(musicNextBeat / MUSIC_CYCLE_BEATS); updateMusicGain(); scheduleMusic(); } }
export function stopMusic() { if (musicTimer !== null) window.clearTimeout(musicTimer); musicTimer = null; updateMusicGain(); }
export function syncMusic() { if (musicPreference && soundEnabled() && !musicHidden) startMusic(); else stopMusic(); }
export function installMusicVisibility(): () => void { const change = () => { musicHidden = document.hidden; syncMusic(); }; document.addEventListener("visibilitychange", change); return () => document.removeEventListener("visibilitychange", change); }

function tone(freq: number, duration: number, type: OscillatorType, peak: number, delay = 0, slideTo?: number) {
  const ctx = audio();
  if (!ctx) return;
  const osc = ctx.createOscillator();
  const gain = ctx.createGain();
  const start = ctx.currentTime + delay;
  osc.type = type;
  osc.frequency.setValueAtTime(freq, start);
  if (slideTo) osc.frequency.exponentialRampToValueAtTime(slideTo, start + duration);
  gain.gain.setValueAtTime(0.0001, start);
  gain.gain.exponentialRampToValueAtTime(peak, start + 0.008);
  gain.gain.exponentialRampToValueAtTime(0.0001, start + duration);
  osc.connect(gain).connect(ctx.destination);
  osc.start(start);
  osc.stop(start + duration + 0.02);
}

function swish(duration: number, peak: number, delay = 0) {
  const ctx = audio();
  if (!ctx) return;
  const source = ctx.createBufferSource();
  source.buffer = noiseBuffer(ctx);
  const filter = ctx.createBiquadFilter();
  filter.type = "bandpass";
  filter.frequency.value = 1900;
  filter.Q.value = 0.8;
  const gain = ctx.createGain();
  const start = ctx.currentTime + delay;
  gain.gain.setValueAtTime(peak, start);
  gain.gain.exponentialRampToValueAtTime(0.0001, start + duration);
  source.connect(filter).connect(gain).connect(ctx.destination);
  source.start(start, 0, Math.min(duration, 0.25));
}

function guard(play: () => void) {
  if (!soundEnabled()) return;
  try {
    play();
  } catch {
    // 오디오가 막힌 환경에서는 조용히 넘어간다.
  }
}

export const sfx = {
  /** 카드 한 장이 테이블에 놓이는 소리. */
  deal: () =>
    guard(() => {
      swish(0.11, 0.35);
      tone(1400, 0.04, "triangle", 0.05, 0.05);
    }),
  /** 칩 하나를 놓는 소리. */
  chip: () =>
    guard(() => {
      tone(2300, 0.05, "sine", 0.18, 0, 1700);
      tone(3200, 0.04, "sine", 0.08, 0.03);
    }),
  /** 베팅 확정: 칩 더미를 미는 소리. */
  lock: () =>
    guard(() => {
      tone(1900, 0.06, "sine", 0.14);
      tone(2400, 0.05, "sine", 0.12, 0.06);
      tone(1500, 0.08, "sine", 0.1, 0.11);
    }),
  /** 내 차례 알림. */
  turn: () =>
    guard(() => {
      tone(880, 0.09, "sine", 0.16);
      tone(1175, 0.12, "sine", 0.16, 0.11);
    }),
  /** 이겼다. */
  win: () =>
    guard(() => {
      [523, 659, 784, 1046].forEach((freq, index) => tone(freq, 0.16, "triangle", 0.16, index * 0.09));
      tone(1568, 0.4, "sine", 0.1, 0.38);
    }),
  /** 졌다. */
  lose: () =>
    guard(() => {
      tone(330, 0.18, "sawtooth", 0.05, 0, 220);
    }),
};
