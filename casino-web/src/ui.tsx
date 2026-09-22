// 원본(noir-casino)이 쓰던 shadcn/radix 컴포넌트의 경량 대체. 같은 data-slot / data-state 속성과
// 클래스 구조를 유지해 원본 globals.css 규칙이 그대로 적용된다.
import {
  createContext,
  useContext,
  useEffect,
  useState,
  type CSSProperties,
  type ReactNode,
} from "react";
import { X } from "lucide-react";

// ------------------------------------------------------------ Tabs

interface TabsContextValue {
  value: string;
  setValue: (value: string) => void;
}
const TabsContext = createContext<TabsContextValue | null>(null);

export function Tabs({
  value,
  defaultValue,
  onValueChange,
  className,
  children,
}: {
  value?: string;
  defaultValue?: string;
  onValueChange?: (value: string) => void;
  className?: string;
  children: ReactNode;
}) {
  const [inner, setInner] = useState(defaultValue ?? "");
  const current = value ?? inner;
  const setValue = (next: string) => {
    if (value === undefined) {
      setInner(next);
    }
    onValueChange?.(next);
  };
  return (
    <TabsContext.Provider value={{ value: current, setValue }}>
      <div data-slot="tabs" data-orientation="horizontal" className={className}>
        {children}
      </div>
    </TabsContext.Provider>
  );
}

export function TabsList({ className, children }: { className?: string; children: ReactNode }) {
  return (
    <div role="tablist" data-slot="tabs-list" data-variant="default" className={className}>
      {children}
    </div>
  );
}

export function TabsTrigger({
  value,
  children,
  "aria-label": ariaLabel,
  title,
}: {
  value: string;
  children: ReactNode;
  "aria-label"?: string;
  title?: string;
}) {
  const context = useContext(TabsContext);
  const active = context?.value === value;
  return (
    <button
      type="button"
      role="tab"
      aria-selected={active}
      aria-label={ariaLabel}
      title={title}
      data-slot="tabs-trigger"
      data-state={active ? "active" : "inactive"}
      onClick={() => context?.setValue(value)}
    >
      {children}
    </button>
  );
}

export function TabsContent({ value, children }: { value: string; children: ReactNode }) {
  const context = useContext(TabsContext);
  if (context?.value !== value) {
    return null;
  }
  return (
    <div role="tabpanel" data-slot="tabs-content">
      {children}
    </div>
  );
}

// ------------------------------------------------------------ Dialog / Sheet

function useEscape(open: boolean, onClose: () => void) {
  useEffect(() => {
    if (!open) {
      return;
    }
    const handler = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        onClose();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [open, onClose]);
}

const CloseContext = createContext<() => void>(() => {});

interface OpenProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  children: ReactNode;
}

export function Dialog({ open, onOpenChange, children }: OpenProps) {
  const close = () => onOpenChange(false);
  useEscape(open, close);
  if (!open) {
    return null;
  }
  return <CloseContext.Provider value={close}>{children}</CloseContext.Provider>;
}

export function DialogContent({ className, children }: { className?: string; children: ReactNode }) {
  const close = useContext(CloseContext);
  return (
    <>
      <div data-slot="dialog-overlay" data-state="open" className="ui-overlay" onClick={close} />
      <div
        role="dialog"
        aria-modal="true"
        data-slot="dialog-content"
        data-state="open"
        className={`ui-dialog ${className ?? ""}`}
      >
        {children}
        <button type="button" data-slot="dialog-close" className="ui-close" aria-label="닫기" onClick={close}>
          <X />
        </button>
      </div>
    </>
  );
}

export function DialogTitle({ children }: { children: ReactNode }) {
  return <h2 data-slot="dialog-title">{children}</h2>;
}

export function DialogDescription({ children }: { children: ReactNode }) {
  return <p data-slot="dialog-description">{children}</p>;
}

export function Sheet({ open, onOpenChange, children }: OpenProps) {
  const close = () => onOpenChange(false);
  useEscape(open, close);
  if (!open) {
    return null;
  }
  return <CloseContext.Provider value={close}>{children}</CloseContext.Provider>;
}

export function SheetContent({ className, children }: { className?: string; children: ReactNode }) {
  const close = useContext(CloseContext);
  return (
    <>
      <div data-slot="sheet-overlay" data-state="open" className="ui-overlay" onClick={close} />
      <div
        role="dialog"
        aria-modal="true"
        data-slot="sheet-content"
        data-state="open"
        className={`ui-sheet ${className ?? ""}`}
      >
        {children}
        <button type="button" data-slot="sheet-close" className="ui-close" aria-label="닫기" onClick={close}>
          <X />
        </button>
      </div>
    </>
  );
}

export function SheetHeader({ children }: { children: ReactNode }) {
  return <div data-slot="sheet-header">{children}</div>;
}

export function SheetTitle({ children }: { children: ReactNode }) {
  return <h2 data-slot="sheet-title">{children}</h2>;
}

export function SheetDescription({ children }: { children: ReactNode }) {
  return <p data-slot="sheet-description">{children}</p>;
}

// ------------------------------------------------------------ Slider

export function Slider({
  value,
  min,
  max,
  step = 1,
  disabled,
  onValueChange,
  className,
  "aria-label": ariaLabel,
}: {
  value: number[];
  min: number;
  max: number;
  step?: number;
  disabled?: boolean;
  onValueChange: (value: number[]) => void;
  className?: string;
  "aria-label"?: string;
}) {
  const current = value[0] ?? min;
  const percent = max > min ? Math.min(100, Math.max(0, ((current - min) / (max - min)) * 100)) : 0;
  return (
    <span
      data-slot="slider"
      data-orientation="horizontal"
      data-disabled={disabled ? "" : undefined}
      className={`ui-slider ${className ?? ""}`}
      style={{ "--p": `${percent}%` } as CSSProperties}
    >
      <input
        type="range"
        aria-label={ariaLabel}
        min={min}
        max={max}
        step={step}
        value={current}
        disabled={disabled}
        onChange={(event) => onValueChange([Number(event.target.value)])}
      />
    </span>
  );
}

// ------------------------------------------------------------ Toaster (sonner 대체)

type ToastKind = "success" | "info" | "error";
interface ToastItem {
  id: number;
  kind: ToastKind;
  message: string;
}

const listeners = new Set<(items: ToastItem[]) => void>();
let toasts: ToastItem[] = [];
let nextToastId = 1;

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

export function Toaster(_props: { position?: string; richColors?: boolean }) {
  const [items, setItems] = useState<ToastItem[]>(toasts);
  useEffect(() => {
    listeners.add(setItems);
    return () => {
      listeners.delete(setItems);
    };
  }, []);
  return (
    <div className="ui-toaster" aria-live="polite">
      {items.map((item) => (
        <div key={item.id} className={`ui-toast ui-toast-${item.kind}`} role="status">
          {item.message}
        </div>
      ))}
    </div>
  );
}
