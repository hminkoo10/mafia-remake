// 작은 UI 부품: 탭, 옆에서 나오는 시트, 알림(토스트).
import { createContext, useContext, useEffect, useState, type ReactNode } from "react";
import { X } from "lucide-react";

// ------------------------------------------------------------ 탭

interface TabsContextValue {
  value: string;
  setValue: (value: string) => void;
}
const TabsContext = createContext<TabsContextValue | null>(null);

export function Tabs({
  value,
  onValueChange,
  className,
  children,
}: {
  value: string;
  onValueChange: (value: string) => void;
  className?: string;
  children: ReactNode;
}) {
  return (
    <TabsContext.Provider value={{ value, setValue: onValueChange }}>
      <div className={`tabs ${className ?? ""}`}>{children}</div>
    </TabsContext.Provider>
  );
}

export function TabsList({ children, label }: { children: ReactNode; label?: string }) {
  return (
    <div role="tablist" aria-label={label} className="tab-list">
      {children}
    </div>
  );
}

export function TabsTrigger({ value, children }: { value: string; children: ReactNode }) {
  const context = useContext(TabsContext);
  const active = context?.value === value;
  return (
    <button type="button" role="tab" aria-selected={active} className={`tab ${active ? "active" : ""}`} onClick={() => context?.setValue(value)}>
      {children}
    </button>
  );
}

export function TabsContent({ value, children }: { value: string; children: ReactNode }) {
  const context = useContext(TabsContext);
  if (context?.value !== value) return null;
  return (
    <div role="tabpanel" className="tab-panel">
      {children}
    </div>
  );
}

// ------------------------------------------------------------ 시트

export function Sheet({
  open,
  onClose,
  title,
  description,
  children,
}: {
  open: boolean;
  onClose: () => void;
  title: string;
  description?: string;
  children: ReactNode;
}) {
  useEffect(() => {
    if (!open) return;
    const handler = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [open, onClose]);
  if (!open) return null;
  return (
    <>
      <div className="sheet-overlay" onClick={onClose} />
      <div role="dialog" aria-modal="true" aria-label={title} className="sheet">
        <div className="sheet-head">
          <div>
            <h2>{title}</h2>
            {description && <p>{description}</p>}
          </div>
          <button type="button" className="icon-button" aria-label="닫기" onClick={onClose}>
            <X size={18} />
          </button>
        </div>
        {children}
      </div>
    </>
  );
}

// ------------------------------------------------------------ 알림

type ToastKind = "success" | "info" | "error";
interface ToastItem {
  id: number;
  kind: ToastKind;
  message: string;
}

let nextToastId = 1;
let toasts: ToastItem[] = [];
const listeners = new Set<(items: ToastItem[]) => void>();

function emit(kind: ToastKind, message: string) {
  const item: ToastItem = { id: nextToastId++, kind, message };
  toasts = [...toasts, item].slice(-4);
  listeners.forEach((listener) => listener(toasts));
  window.setTimeout(() => {
    toasts = toasts.filter((entry) => entry.id !== item.id);
    listeners.forEach((listener) => listener(toasts));
  }, 3800);
}

export const toast = {
  success: (message: string) => emit("success", message),
  info: (message: string) => emit("info", message),
  error: (message: string) => emit("error", message),
};

export function Toaster() {
  const [items, setItems] = useState<ToastItem[]>(toasts);
  useEffect(() => {
    listeners.add(setItems);
    return () => {
      listeners.delete(setItems);
    };
  }, []);
  return (
    <div className="toaster" aria-live="polite">
      {items.map((item) => (
        <div key={item.id} className={`toast ${item.kind}`} role="status">
          {item.message}
        </div>
      ))}
    </div>
  );
}
