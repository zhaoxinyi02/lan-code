import React from "react";
import { createPortal } from "react-dom";
import { Check, ChevronDown } from "lucide-react";

export type SelectOption<T extends string = string> = {
  id: T;
  label: string;
  description?: string;
};

export default function SelectMenu<T extends string>({
  value,
  options,
  onChange,
  placeholder = "请选择",
  ariaLabel,
  compact = false,
  className = "",
}: {
  value: T;
  options: SelectOption<T>[];
  onChange: (value: T) => void;
  placeholder?: string;
  ariaLabel?: string;
  compact?: boolean;
  className?: string;
}) {
  const [open, setOpen] = React.useState(false);
  const [menuStyle, setMenuStyle] = React.useState<React.CSSProperties>({});
  const host = React.useRef<HTMLDivElement>(null);
  const menu = React.useRef<HTMLDivElement>(null);
  const selected = options.find((option) => option.id === value);

  const updatePosition = React.useCallback(() => {
    if (!host.current) return;
    const rect = host.current.getBoundingClientRect();
    const menuWidth = Math.min(Math.max(rect.width, 220), window.innerWidth - 16);
    const availableBelow = window.innerHeight - rect.bottom - 12;
    const availableAbove = rect.top - 12;
    const openUpward = availableBelow < 220 && availableAbove > availableBelow;
    setMenuStyle({
      top: openUpward ? undefined : rect.bottom + 6,
      bottom: openUpward ? window.innerHeight - rect.top + 6 : undefined,
      left: Math.max(8, Math.min(rect.left, window.innerWidth - menuWidth - 8)),
      width: menuWidth,
      maxHeight: Math.max(120, Math.min(340, openUpward ? availableAbove : availableBelow)),
    });
  }, []);

  React.useEffect(() => {
    const close = (event: PointerEvent) => {
      const target = event.target as Node;
      if (!host.current?.contains(target) && !menu.current?.contains(target)) setOpen(false);
    };
    const reposition = () => {
      if (open) requestAnimationFrame(updatePosition);
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    window.addEventListener("pointerdown", close);
    window.addEventListener("keydown", closeOnEscape);
    window.addEventListener("resize", reposition);
    window.addEventListener("scroll", reposition, true);
    return () => {
      window.removeEventListener("pointerdown", close);
      window.removeEventListener("keydown", closeOnEscape);
      window.removeEventListener("resize", reposition);
      window.removeEventListener("scroll", reposition, true);
    };
  }, [open, updatePosition]);

  const toggle = () => {
    if (!open) updatePosition();
    setOpen((current) => !current);
  };

  return <div className={`select-menu ${compact ? "compact" : ""} ${className}`} ref={host}>
    <button
      type="button"
      className={`select-menu-trigger ${open ? "open" : ""}`}
      aria-label={ariaLabel}
      aria-haspopup="listbox"
      aria-expanded={open}
      onClick={toggle}
    >
      <span>{selected?.label || placeholder}</span>
      <ChevronDown size={14} />
    </button>
    {open && createPortal(
      <div className="select-menu-popover" ref={menu} style={menuStyle} role="listbox" onPointerDown={(event) => event.stopPropagation()}>
        {options.length === 0
          ? <div className="select-menu-empty">暂无可选项</div>
          : options.map((option) => <button
            type="button"
            role="option"
            aria-selected={option.id === value}
            className={option.id === value ? "active" : ""}
            key={option.id}
            onPointerDown={(event) => {
              event.preventDefault();
              onChange(option.id);
              setOpen(false);
            }}
          >
            <span><strong>{option.label}</strong>{option.description && <small>{option.description}</small>}</span>
            {option.id === value && <Check size={14} />}
          </button>)}
      </div>,
      document.body,
    )}
  </div>;
}
