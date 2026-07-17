export function formatBeijingDateTimeSeconds(
  timestampSeconds: number | null,
  locale: string,
): string | null {
  if (timestampSeconds == null) {
    return null;
  }

  return new Intl.DateTimeFormat(locale, {
    dateStyle: "medium",
    timeStyle: "short",
    timeZone: "Asia/Shanghai",
  }).format(new Date(timestampSeconds * 1000));
}
