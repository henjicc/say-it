import { HelpLabel } from "./Tooltip";
import { cn } from "@/lib/cn";

/** 分组标题：分组面上方的一行小号标题，可选右侧操作区。 */
export function SectionHeader({
  title,
  description,
  right,
  className,
}: {
  title: React.ReactNode;
  description?: React.ReactNode;
  right?: React.ReactNode;
  className?: string;
}) {
  return (
    <div className={cn("flex min-h-7 items-center gap-3 px-0.5", className)}>
      <h2 className="flex-1 text-xs font-normal text-[var(--color-fg-subtle)]"><HelpLabel content={description}>{title}</HelpLabel></h2>
      {right && <div className="flex flex-none items-center gap-2">{right}</div>}
    </div>
  );
}
