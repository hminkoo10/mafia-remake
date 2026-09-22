import { memo, useLayoutEffect, useRef } from "react";
import type { ChatMessage } from "../types";

export const TableChatPreview = memo(function TableChatPreview({ messages }: { messages: ChatMessage[] }) {
  const recent = messages.filter((message) => !message.dealer).slice(-3);
  return (
    <div className="table-chat-preview" aria-label="테이블 위 최근 채팅">
      <span className="table-chat-label">테이블 채팅</span>
      {recent.length ? recent.map((message) => (
        <p key={message.id}><strong>{message.name}</strong><span>{message.text}</span></p>
      )) : <p className="table-chat-empty">함께하는 플레이어에게 인사해 보세요.</p>}
    </div>
  );
});

export const ChatMessages = memo(function ChatMessages({ messages, dealerName }: { messages: ChatMessage[]; dealerName: string }) {
  const scroller = useRef<HTMLDivElement>(null);
  const follow = useRef(true);
  // 40개 고정 길이의 채팅에서도 마지막 메시지가 바뀌면 따라간다. 페이지 자체는 스크롤하지 않는다.
  useLayoutEffect(() => {
    const element = scroller.current;
    if (element && follow.current) element.scrollTop = element.scrollHeight;
  }, [messages[messages.length - 1]?.id]);
  return (
    <div ref={scroller} className="chat-messages" role="log" aria-label="테이블 채팅" onScroll={(event) => {
      const element = event.currentTarget;
      follow.current = element.scrollHeight - element.scrollTop - element.clientHeight < 48;
    }}>
      {messages.length ? messages.map((message) => (
        <div key={message.id} className={`chat-message ${message.dealer ? "from-dealer" : ""}`}>
          <strong>{message.name}{message.dealer && <span>DEALER</span>}{message.from_discord && <span>DISCORD</span>}</strong>
          <p>{message.text}</p>
        </div>
      )) : <div className="chat-empty"><p>{dealerName}와 함께하는 테이블.<br />첫 인사를 건네보세요.</p></div>}
    </div>
  );
});
