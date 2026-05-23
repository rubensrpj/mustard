import * as React from "react"
import { cva, type VariantProps } from "class-variance-authority"
import { Slot } from "radix-ui"

import { cn } from "@/lib/utils"

const badgeVariants = cva(
  "group/badge inline-flex h-5 w-fit shrink-0 items-center justify-center gap-1 overflow-hidden rounded-4xl border border-transparent px-2 py-0.5 text-xs font-medium whitespace-nowrap transition-all focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50 has-data-[icon=inline-end]:pr-1.5 has-data-[icon=inline-start]:pl-1.5 aria-invalid:border-destructive aria-invalid:ring-destructive/20 dark:aria-invalid:ring-destructive/40 [&>svg]:pointer-events-none [&>svg]:size-3!",
  {
    variants: {
      variant: {
        default: "bg-primary text-primary-foreground [a]:hover:bg-primary/80",
        secondary:
          "bg-secondary text-secondary-foreground [a]:hover:bg-secondary/80",
        destructive:
          "bg-destructive/10 text-destructive focus-visible:ring-destructive/20 dark:bg-destructive/20 dark:focus-visible:ring-destructive/40 [a]:hover:bg-destructive/20",
        outline:
          "border-border text-foreground [a]:hover:bg-muted [a]:hover:text-muted-foreground",
        ghost:
          "hover:bg-muted hover:text-muted-foreground dark:hover:bg-muted/50",
        link: "text-primary underline-offset-4 hover:underline",
        "tag-purple":
          "rounded-sm bg-primary/10 text-primary dark:bg-primary/15 dark:text-primary",
        "tag-orange":
          "rounded-sm bg-orange-100 text-orange-700 dark:bg-orange-500/15 dark:text-orange-300",
        "tag-green":
          "rounded-sm bg-[--intent-success]/10 text-[--intent-success] dark:bg-[--intent-success]/15",
        success:
          "bg-emerald-100 text-emerald-700 dark:bg-emerald-500/15 dark:text-emerald-300",
        warning:
          "bg-amber-100 text-amber-700 dark:bg-amber-500/15 dark:text-amber-300",
        error:
          "bg-red-100 text-[--intent-error] dark:bg-[--intent-error]/15/15 dark:text-red-300",
        info: "bg-sky-100 text-sky-700 dark:bg-sky-500/15 dark:text-sky-300",
        "status-draft":
          "bg-sky-100 text-sky-700 dark:bg-sky-500/15 dark:text-sky-300",
        "status-implementing":
          "bg-amber-100 text-amber-700 dark:bg-amber-500/15 dark:text-amber-300",
        "status-awaiting-qa":
          "bg-amber-100 text-amber-700 border border-amber-400/40 dark:bg-amber-500/15 dark:text-amber-300",
        "status-completed":
          "bg-emerald-100 text-emerald-700 dark:bg-emerald-500/15 dark:text-emerald-300",
      },
    },
    defaultVariants: {
      variant: "default",
    },
  }
)

function Badge({
  className,
  variant = "default",
  asChild = false,
  ...props
}: React.ComponentProps<"span"> &
  VariantProps<typeof badgeVariants> & { asChild?: boolean }) {
  const Comp = asChild ? Slot.Root : "span"

  return (
    <Comp
      data-slot="badge"
      data-variant={variant}
      className={cn(badgeVariants({ variant }), className)}
      {...props}
    />
  )
}

export { Badge, badgeVariants }
