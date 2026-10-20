import { Children, Fragment, isValidElement, type ReactNode } from "react";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "./select";
import { cn } from "@/lib/utils";

interface Option {
  value: string;
  label: ReactNode;
  disabled?: boolean;
}

function readOptions(children: ReactNode): Option[] {
  const options: Option[] = [];
  Children.forEach(children, (child) => {
    if (
      !isValidElement<{
        value?: string | number;
        children?: ReactNode;
        disabled?: boolean;
      }>(child)
    )
      return;
    if (child.type === Fragment) {
      options.push(...readOptions(child.props.children));
    } else if (child.type === "option") {
      options.push({
        value: String(
          child.props.value ?? Children.toArray(child.props.children).join(""),
        ),
        label: child.props.children,
        disabled: child.props.disabled,
      });
    }
  });
  return options;
}

/** The shared themed dropdown, with the existing option-based authoring API. */
export function OptionSelect({
  children,
  value,
  onValueChange,
  className,
  disabled,
  ...props
}: {
  children: ReactNode;
  value?: string | number;
  onValueChange: (value: string) => void;
  className?: string;
  disabled?: boolean;
  id?: string;
  title?: string;
  "aria-label"?: string;
  "aria-labelledby"?: string;
}) {
  const options = readOptions(children);
  // Prefix values so empty-string options retain their original meaning in Radix.
  return (
    <Select
      value={`option:${value ?? ""}`}
      onValueChange={(next) => onValueChange(next.slice("option:".length))}
      disabled={disabled}
    >
      <SelectTrigger {...props} className={cn("w-auto max-w-full", className)}>
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        {options.map((option) => (
          <SelectItem
            key={option.value}
            value={`option:${option.value}`}
            disabled={option.disabled}
          >
            {option.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}
