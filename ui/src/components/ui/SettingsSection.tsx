import { cn } from "@/lib/cn";
import { SectionHeader } from "./SectionHeader";

/** 设置区块：分组标题 + 内容，统一分组之间的间距节奏。 */
export function SettingsSection({
  title,
  right,
  children,
  className,
  bodyClassName,
}: {
  title: React.ReactNode;
  right?: React.ReactNode;
  children: React.ReactNode;
  className?: string;
  bodyClassName?: string;
}) {
  return (
    <section className={cn("flex flex-col gap-4", className)}>
      <SectionHeader title={title} right={right} />
      <div className={cn("flex flex-col gap-4", bodyClassName)}>{children}</div>
    </section>
  );
}
