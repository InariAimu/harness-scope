import { useEffect, useState } from "react";
import { Input, type InputProps } from "@/components/ui/input";

/** Commit complete text on blur/Enter, including IME input. Choice controls
 * save on selection; incomplete text must not be persisted on each keystroke. */
export function AutoSaveInput({ value, onCommit, ...props }: Omit<InputProps, "value" | "onChange"> & {
  value: string | number;
  onCommit: (value: string) => void;
}) {
  const [text, setText] = useState(String(value));
  useEffect(() => setText(String(value)), [value]);
  return <Input {...props} value={text}
    onChange={(event) => setText(event.target.value)}
    onBlur={() => { if (text !== String(value)) onCommit(text); }}
    onKeyDown={(event) => {
      if (event.key === "Enter" && !event.nativeEvent.isComposing) event.currentTarget.blur();
    }}
  />;
}
