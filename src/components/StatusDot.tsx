import { cn } from "@/lib/utils";

export type StatusDotVariant = "idle" | "active" | "planning" | "blocked" | "done";

interface StatusDotProps {
  variant: StatusDotVariant;
  pulse?: boolean;
  size?: "sm" | "md";
  className?: string;
}

const COLOR: Record<StatusDotVariant, string> = {
  idle: "bg-zinc-500",
  active: "bg-emerald-500",
  planning: "bg-amber-500",
  blocked: "bg-rose-500",
  done: "bg-zinc-400",
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
        variant === "active" && "ring-1 ring-emerald-500/30",
        pulse && variant === "active" && "animate-pulse",
        className,
      )}
    />
  );
}
