export function youtubeErrorMessage(code: number): string {
  switch (code) {
    case 2:
      return "The YouTube video ID is invalid.";
    case 5:
      return "This YouTube video cannot be played in the embedded player.";
    case 100:
      return "This YouTube video is unavailable.";
    case 101:
    case 150:
      return "The owner does not allow this YouTube video to play in embedded players.";
    case 153:
      return "YouTube rejected the embedded player request because it could not identify Ganbaru as the embedding client.";
    default:
      return `YouTube playback failed with error ${code}.`;
  }
}
