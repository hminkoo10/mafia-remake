export type MusicNoteKind = "chord" | "bass" | "shaker" | "vibe";
export type MusicNote = { kind: MusicNoteKind; beat: number; duration: number; frequency?: number; gain: number };
export const MUSIC_BPM = 80;
export const MUSIC_BEATS_PER_BAR = 4;
export const MUSIC_BARS_PER_CYCLE = 8;
export const MUSIC_CYCLE_BEATS = MUSIC_BEATS_PER_BAR * MUSIC_BARS_PER_CYCLE;
export const MUSIC_LOOK_AHEAD_SECONDS = 0.3;
export const MUSIC_MASTER_GAIN = 0.0794;

const progressions = [
  [[261.63, 329.63, 392, 493.88], [220, 261.63, 329.63, 440], [174.61, 220, 261.63, 329.63], [196, 246.94, 293.66, 392]],
  [[293.66, 369.99, 440, 554.37], [246.94, 293.66, 369.99, 493.88], [196, 246.94, 293.66, 392], [220, 277.18, 329.63, 440]],
  [[261.63, 329.63, 392, 493.88], [196, 246.94, 293.66, 392], [220, 277.18, 329.63, 440], [246.94, 293.66, 369.99, 493.88]],
] as const;

export function progressionForCycle(cycle: number): readonly (readonly number[])[] {
  return progressions[Math.abs(Math.floor(cycle)) % progressions.length];
}

export function musicNotesForCycle(cycle: number): MusicNote[] {
  const chords = progressionForCycle(cycle);
  const notes: MusicNote[] = [];
  for (let bar = 0; bar < MUSIC_BARS_PER_CYCLE; bar++) {
    const beat = bar * MUSIC_BEATS_PER_BAR;
    const chord = chords[bar % chords.length];
    chord.forEach((frequency, index) => notes.push({ kind: "chord", beat, duration: 3.6, frequency, gain: index === 0 ? 0.18 : 0.11 }));
    [chord[0] / 2, chord[0] / 2, chord[1] / 2].forEach((frequency, index) => notes.push({ kind: "bass", beat: beat + index * 1.5, duration: 1.15, frequency, gain: 0.22 }));
    for (let step = 0; step < MUSIC_BEATS_PER_BAR; step += 0.5) notes.push({ kind: "shaker", beat: beat + step, duration: 0.16, gain: step % 1 ? 0.07 : 0.045 });
    if ((bar + Math.abs(cycle)) % 3 === 1) notes.push({ kind: "vibe", beat: beat + 2.5, duration: 1.1, frequency: chord[2] * 2, gain: 0.12 });
  }
  return notes.sort((a, b) => a.beat - b.beat || a.kind.localeCompare(b.kind));
}

export function notesInWindow(notes: readonly MusicNote[], fromBeat: number, toBeat: number): MusicNote[] {
  return notes.filter((note) => note.beat >= fromBeat && note.beat < toBeat);
}
