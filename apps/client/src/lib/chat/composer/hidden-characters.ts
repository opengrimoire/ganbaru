/** Text with hidden characters removed, and how many were removed. */
export interface SanitizedComposerText {
  text: string;
  hiddenCharacters: number;
}

const ZERO_WIDTH_JOINER = 0x200d;
const COMBINING_ENCLOSING_KEYCAP = 0x20e3;
const TEXT_PRESENTATION_SELECTOR = 0xfe0e;
const EMOJI_PRESENTATION_SELECTOR = 0xfe0f;

/**
 * Characters that are always removed: control characters except tab and newline, the soft
 * hyphen, bidirectional embeddings, overrides, and isolates, zero-width characters other than the
 * joiner, variation selectors other than the two emoji presentation selectors, and tag characters.
 * Hidden characters let a reader see one text while an agent reads another.
 */
const ALWAYS_HIDDEN = /[\u{0}-\u{8}\u{B}\u{C}\u{E}-\u{1F}\u{7F}-\u{9F}\u{AD}\u{200B}\u{200C}\u{202A}-\u{202E}\u{2060}\u{2066}-\u{2069}\u{FE00}-\u{FE0D}\u{FEFF}\u{E0000}-\u{E007F}\u{E0100}-\u{E01EF}]/gu;
const PICTOGRAPHIC = /^\p{Extended_Pictographic}$/u;
const EMOJI_MODIFIER = /^\p{Emoji_Modifier}$/u;
const KEYCAP_BASE = /^[0-9#*]$/u;

/**
 * Removes hidden characters from text entering the composer from outside, such as a paste or a
 * drop, and normalizes line endings to newlines.
 *
 * Every other character is kept as it is, including surrounding whitespace. A zero-width joiner
 * stays only between two emoji parts, and an emoji presentation selector stays only after an
 * emoji or a keycap base, so emoji sequences keep rendering as one emoji.
 *
 * @param raw The incoming text.
 * @returns The visible text and how many hidden characters were removed; a line ending
 *   normalization is not counted.
 */
export function sanitizeComposerText(raw: string): SanitizedComposerText {
  let hiddenCharacters = 0;
  const normalized = raw.replace(/\r\n?/gu, "\n").replace(ALWAYS_HIDDEN, () => {
    hiddenCharacters += 1;
    return "";
  });
  const characters = Array.from(normalized);
  const kept = characters.filter((character, index) => {
    const codePoint = character.codePointAt(0);
    if (codePoint === ZERO_WIDTH_JOINER) {
      const keep = isEmojiPart(characters[index - 1]) && isEmojiPart(characters[index + 1]);
      if (!keep) hiddenCharacters += 1;
      return keep;
    }
    if (codePoint === TEXT_PRESENTATION_SELECTOR || codePoint === EMOJI_PRESENTATION_SELECTOR) {
      const keep = selectsEmojiPresentation(characters[index - 1], characters[index + 1]);
      if (!keep) hiddenCharacters += 1;
      return keep;
    }
    return true;
  });
  return { text: kept.join(""), hiddenCharacters };
}

function isEmojiPart(character: string | undefined): boolean {
  if (character === undefined) return false;
  return PICTOGRAPHIC.test(character)
    || EMOJI_MODIFIER.test(character)
    || character.codePointAt(0) === EMOJI_PRESENTATION_SELECTOR;
}

function selectsEmojiPresentation(previous: string | undefined, next: string | undefined): boolean {
  if (previous === undefined) return false;
  if (PICTOGRAPHIC.test(previous)) return true;
  return KEYCAP_BASE.test(previous) && next?.codePointAt(0) === COMBINING_ENCLOSING_KEYCAP;
}
