import { cn } from "@/lib/cn";
import { Button, type ButtonProps } from "./Button";

export interface IconButtonProps extends Omit<ButtonProps, "aria-label"> {
  label: string;
}

/** 仅图标操作按钮：统一方形尺寸，并强制提供可访问名称。 */
export function IconButton({ label, title, size = "md", className, ...props }: IconButtonProps) {
  return (
    <Button
      aria-label={label}
      title={title ?? label}
      size={size}
      className={cn(
        size === "sm" ? "w-[var(--control-h-sm)] px-0" : "w-[var(--control-h)] px-0",
        className,
      )}
      {...props}
    />
  );
}
