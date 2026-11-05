/** Characters a masked code keeps visible at each end, enough to compare it with its source. */
export const MASKED_CODE_EDGE = 4;

/** Fixed mark standing in for the hidden middle, so the mask does not reveal how long the code is. */
const MASK_MARK = "••••";

/** A lowercase scheme such as `ganbaru-anchor:` names the kind of code and is not secret. */
const CODE_SCHEME = /^[a-z][a-z0-9-]*:/;

/**
 * Shows only the ends of a secret code: a leading scheme stays whole, then the first and last
 * {@link MASKED_CODE_EDGE} characters of the rest surround a fixed mark. A code too short to hide
 * at least as many characters as each end shows is returned unchanged.
 *
 * @param code The code as pasted, already trimmed.
 * @returns The text to display in place of the code.
 */
export function maskCode(code: string): string {
  const scheme = CODE_SCHEME.exec(code)?.[0] ?? "";
  const body = [...code.slice(scheme.length)];
  if (body.length < MASKED_CODE_EDGE * 3) return code;
  return `${scheme}${body.slice(0, MASKED_CODE_EDGE).join("")}${MASK_MARK}${body.slice(-MASKED_CODE_EDGE).join("")}`;
}
