// 뉴스·공시 목록 (종목 누르면 그 종목으로).
import { NEWS_KIND_TEXT, relativeText } from "./format";
import type { NewsItem } from "./types";

export function NewsList({
  items,
  now,
  empty,
  onSelect,
}: {
  items: NewsItem[];
  now: number;
  empty: string;
  onSelect?: (code: string) => void;
}) {
  if (items.length === 0) return <p className="hts-empty">{empty}</p>;
  return (
    <ul className="hts-news">
      {items.map((item) => (
        <li key={item.id} className={item.tone > 0 ? "good" : item.tone < 0 ? "bad" : ""}>
          <div>
            <span className={`news-kind ${item.kind}`}>{NEWS_KIND_TEXT[item.kind] ?? item.kind}</span>
            <time>{relativeText(item.at, now)}</time>
            {onSelect && item.code && (
              <button className="text-button" onClick={() => onSelect(item.code!)}>
                종목 보기
              </button>
            )}
          </div>
          <p>{item.headline}</p>
          {item.body && <small>{item.body}</small>}
        </li>
      ))}
    </ul>
  );
}
