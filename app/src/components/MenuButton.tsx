import { useEffect, useId, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";

export interface MenuAction {
  label: string;
  onSelect: () => void;
  icon?: ReactNode;
  disabled?: boolean;
  checked?: boolean;
}

interface Props {
  label: string;
  items: MenuAction[];
  icon?: ReactNode;
  iconOnly?: boolean;
  className?: string;
  disabled?: boolean;
  onOpen?: () => void;
}

/** This menu stays reachable outside clipped toolbars and panels. */
export function MenuButton({ label, items, icon, iconOnly = false, className = "", disabled = false, onOpen }: Props) {
  const [open, setOpen] = useState(false);
  const [position, setPosition] = useState({ top: 0, left: 0 });
  const trigger = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const id = useId();
  const close = (restoreFocus = false) => {
    setOpen(false);
    if (restoreFocus) trigger.current?.focus();
  };

  useLayoutEffect(() => {
    if (!open || !trigger.current || !menu.current) return;
    const anchor = trigger.current.getBoundingClientRect();
    const popup = menu.current.getBoundingClientRect();
    setPosition({
      left: Math.max(8, Math.min(anchor.right - popup.width, window.innerWidth - popup.width - 8)),
      top: anchor.bottom + popup.height + 4 <= window.innerHeight - 8
        ? anchor.bottom + 4
        : Math.max(8, anchor.top - popup.height - 4),
    });
    menu.current.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
  }, [open, items.length]);

  useEffect(() => {
    if (!open) return;
    const outside = (event: PointerEvent) => {
      const target = event.target as Node;
      if (!menu.current?.contains(target) && !trigger.current?.contains(target)) close();
    };
    const moved = (event: Event) => {
      if (!(event.target instanceof Node) || !menu.current?.contains(event.target)) close();
    };
    document.addEventListener("pointerdown", outside);
    window.addEventListener("resize", moved);
    document.addEventListener("scroll", moved, true);
    return () => {
      document.removeEventListener("pointerdown", outside);
      window.removeEventListener("resize", moved);
      document.removeEventListener("scroll", moved, true);
    };
  }, [open]);

  return (
    <>
      <button
        ref={trigger}
        type="button"
        className={`menu-button ${iconOnly ? "icon-button" : ""} ${className}`}
        aria-label={label}
        title={label}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? id : undefined}
        disabled={disabled}
        onMouseDown={(event) => event.preventDefault()}
        onClick={() => {
          if (!open) onOpen?.();
          setOpen(!open);
        }}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown" || event.key === "ArrowUp") {
            event.preventDefault();
            if (!open) onOpen?.();
            setOpen(true);
          }
          if (event.key === "Escape") close(true);
        }}
      >
        {icon}
        {!iconOnly && <span className="menu-label">{label}</span>}
      </button>
      {open && createPortal(
        <div
          ref={menu}
          id={id}
          className="action-menu"
          role="menu"
          aria-label={label}
          style={position}
          onBlur={(event) => {
            if (!event.currentTarget.contains(event.relatedTarget)) close();
          }}
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              event.preventDefault();
              event.stopPropagation();
              close(true);
            } else if (event.key === "Tab") {
              close(true);
            } else if (["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
              event.preventDefault();
              const buttons = Array.from(event.currentTarget.querySelectorAll<HTMLButtonElement>("button:not(:disabled)"));
              const current = buttons.indexOf(document.activeElement as HTMLButtonElement);
              const index = event.key === "Home" ? 0 : event.key === "End" ? buttons.length - 1
                : (current + (event.key === "ArrowDown" ? 1 : -1) + buttons.length) % buttons.length;
              buttons[index]?.focus();
            }
          }}
        >
          {items.map((item, index) => (
            <button
              key={index}
              type="button"
              role={item.checked === undefined ? "menuitem" : "menuitemcheckbox"}
              aria-checked={item.checked}
              disabled={item.disabled}
              title={item.label}
              onMouseDown={(event) => event.preventDefault()}
              onClick={() => {
                close(true);
                item.onSelect();
              }}
            >
              {item.icon}
              <span>{item.label}</span>
              {item.checked && <span className="menu-check" aria-hidden="true">✓</span>}
            </button>
          ))}
        </div>,
        document.body,
      )}
    </>
  );
}
