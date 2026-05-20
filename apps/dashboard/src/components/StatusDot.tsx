import { cn } from "@/lib/utils";

export type StatusDotVariant = "idle" | "active" | "planning" | "blocked" | "done" | "success" | "error";

interface StatusDotProps {
  variant: StatusDotVariant;
  pulse?: boolean;
  size?: "sm" | "md";
  className?: string;
}

const COLOR: Record<StatusDotVariant, string> = {
  idle: "bg-zinc-500",
  active: "bg-[--color-ok]",
  planning: "bg-[--color-accent-mustard]",
  blocked: "bg-[--color-error]",
  done: "bg-zinc-400",
  success: "bg-[--color-ok]",
  error: "bg-[--color-error]",
};

const SIZE: Record<NonNullable<StatusDotProps["size"]>, string> = {
  sm: "w-1.5 h-1.5",
  md: "w-2 h-2",
};

export function StatusDot({ variant, pulse, size = "md", className }: StatusDotProps) {
  return (
    <span
      aria-hidden
      className={cn(
        "inline-block rounded-full",
        SIZE[size],
        COLOR[variant],
        variant === "active" && "ring-1 ring-[--color-ok]/30",
        pulse && variant === "active" && "animate-pulse",
        className,
      )}
    />
  );
}
