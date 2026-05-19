/**
 * Shared page primitives — single source of truth for cross-route visual
 * consistency. Every dashboard page should compose these instead of crafting
 * its own card/header/chip styles.
 *
 * Adding a new page? Import from here:
 *
 *   import {
 *     PageHeader, SectionHeader, KPICard, EmptyState,
 *     DataCard, PhaseChip, EventChip, AcBreakdown,
 *     WaveRowLabel, CollapsibleGroup,
 *   } from "@/components/page";
 *
 * Adding a new visual primitive? Add it here AND drop it in this index so
 * every page picks it up automatically.
 */

export { PageHeader, type PageHeaderProps } from "./PageHeader";
export { SectionHeader, type SectionHeaderProps } from "./SectionHeader";
export { KPICard, type KPICardProps, type KPIAccent } from "./KPICard";
export { EmptyState, type EmptyStateProps, type EmptyVariant } from "./EmptyState";
export { DataCard, type DataCardProps } from "./DataCard";
export { PhaseChip, type PhaseChipProps } from "./PhaseChip";
export { EventChip, type EventChipProps } from "./EventChip";
export { AcBreakdown, type AcBreakdownProps } from "./AcBreakdown";
export { WaveRowLabel, type WaveRowLabelProps } from "./WaveRowLabel";
export { CollapsibleGroup, type CollapsibleGroupProps } from "./CollapsibleGroup";
