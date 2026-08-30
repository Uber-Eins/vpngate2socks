import { useId, type InputHTMLAttributes, type ReactNode, type SelectHTMLAttributes } from "react";

import { cx } from "../utils/cx";
import { ChevronDownIcon } from "./Icon";

export interface SelectOption {
  value: string;
  label: string;
}

/**
 * Label plus native `<select>`. The native control keeps the popup keyboard- and
 * screen-reader-native, and avoids a listbox library that would position its
 * popup with inline styles the served CSP forbids.
 */
export function SelectField({ label, value, options, disabled, onChange, className }: {
  label: string;
  value: string;
  options: SelectOption[];
  disabled?: boolean;
  onChange: (value: string) => void;
  className?: string;
}) {
  const id = useId();
  return (
    <div className={cx("field", className)}>
      <label className="field__label" htmlFor={id}>{label}</label>
      <Select
        id={id}
        value={value}
        disabled={disabled === true}
        onChange={(event) => onChange(event.target.value)}
      >
        {options.map((option) => (
          <option key={option.value} value={option.value}>{option.label}</option>
        ))}
      </Select>
    </div>
  );
}

export function Select({ children, className, ...rest }: SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <span className={cx("select", className)}>
      <select {...rest}>{children}</select>
      <ChevronDownIcon className="select__chevron" size={14} />
    </span>
  );
}

export function TextField({ label, hint, className, ...rest }: InputHTMLAttributes<HTMLInputElement> & {
  label: string;
  hint?: string;
}) {
  const id = useId();
  return (
    <div className={cx("field", className)}>
      <label className="field__label" htmlFor={id}>{label}</label>
      <input {...rest} id={id} className="input" />
      {hint !== undefined && <span className="field__hint">{hint}</span>}
    </div>
  );
}

export function Switch({ checked, disabled, onChange, title, description }: {
  checked: boolean;
  disabled?: boolean;
  onChange: (checked: boolean) => void;
  title: string;
  description?: ReactNode;
}) {
  return (
    <label className="switch">
      <input
        type="checkbox"
        role="switch"
        checked={checked}
        disabled={disabled === true}
        onChange={(event) => onChange(event.target.checked)}
      />
      <span className="switch__track" aria-hidden="true" />
      <span className="switch__text">
        <strong>{title}</strong>
        {description !== undefined && <span>{description}</span>}
      </span>
    </label>
  );
}
