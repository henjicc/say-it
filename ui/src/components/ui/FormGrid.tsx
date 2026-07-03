import { cn } from "@/lib/cn";

/** 表单栅格：桌面端两列（或单列）高密度布局，配合 Field 使用。 */
export function FormGrid({
  columns = 2,
  children,
  className,
}: {
  columns?: 1 | 2;
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <div
      className={cn(
        "grid grid-cols-1 gap-x-6 gap-y-5",
        columns === 2 && "sm:grid-cols-2",
        className,
      )}
    >
      {children}
    </div>
  );
}
