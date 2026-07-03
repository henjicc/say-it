import { cn } from "@/lib/cn";

export function Card({
  className,
  children,
  ...props
}: React.HTMLAttributes<HTMLDivElement>) {
  // 扁平区块：无卡片边框/底色，内容直接平铺；段落之间用细分隔线区分。
  return (
    <section
      className={cn(
        "border-t border-[var(--color-line)] pt-6 first:border-t-0 first:pt-0",
        className,
      )}
      {...props}
    >
      {children}
    </section>
  );
}

export function CardTitle({
  className,
  children,
  ...props
}: React.HTMLAttributes<HTMLHeadingElement>) {
  return (
    <h2 className={cn("text-lg font-semibold tracking-tight text-[var(--color-fg)]", className)} {...props}>
      {children}
    </h2>
  );
}

export function CardDescription({
  className,
  children,
  ...props
}: React.HTMLAttributes<HTMLParagraphElement>) {
  return (
    <p className={cn("mt-1 text-sm leading-relaxed text-[var(--color-fg-subtle)]", className)} {...props}>
      {children}
    </p>
  );
}
