import { useEffect, useState } from "react";
import { Input, type InputProps } from "./input";
import { parseCalendarDate } from "./calendar";

interface DateInputProps
  extends Omit<InputProps, "type" | "value" | "onChange"> {
  value: string;
  onValueChange: (value: string) => void;
}

/** Editable ISO date that uses our calendar instead of a native date popup. */
export function DateInput({
  value,
  onValueChange,
  onBlur,
  ...props
}: DateInputProps) {
  const [text, setText] = useState(value);
  useEffect(() => setText(value), [value]);
  return (
    <Input
      {...props}
      type="text"
      placeholder="YYYY-MM-DD"
      maxLength={10}
      value={text}
      aria-invalid={
        props["aria-invalid"] || (!!text && !parseCalendarDate(text))
      }
      onChange={(event) => {
        const next = event.target.value;
        setText(next);
        if (!next || parseCalendarDate(next)) onValueChange(next);
      }}
      onBlur={(event) => {
        setText(value);
        onBlur?.(event);
      }}
    />
  );
}
