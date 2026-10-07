// 标题与正文分别测量；按字素截断，保留组合 emoji 和附加符号。
export function fitBubbleText(text: string, maxWidth: number, measure: (value: string) => number): string {
  const clean = text.replace(/\s+/gu, " ").trim();
  if (maxWidth <= 0) return "";
  if (measure(clean) <= maxWidth) return clean;
  if (measure("…") > maxWidth) return "";
  const segments = Array.from(new Intl.Segmenter(undefined, { granularity: "grapheme" }).segment(clean), s => s.segment);
  let low = 0, high = segments.length;
  while (low < high) {
    const mid = Math.ceil((low + high) / 2);
    if (measure(segments.slice(0, mid).join("") + "…") <= maxWidth) low = mid;
    else high = mid - 1;
  }
  return segments.slice(0, low).join("") + "…";
}
