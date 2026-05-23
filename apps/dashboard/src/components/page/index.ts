/**
 * Shared page primitives — single source of truth for cross-route visual
 * consistency. Every dashboard page should compose these instead of crafting
 * its own card/header/chip styles.
 *
 * Adding a new page? Import from here:
 *
 *   import {
 *     PageSurface, PageHeader, SectionHeader, KPICard, EmptyState,
 *     DataCard, PhaseChip, EventChip, AcBreakdown,
 *     WaveRowLabel, CollapsibleGroup,
 *     StatPill, BaseRow, CodeBlock, DiffViewer, TreeNode,
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

// Primitives migrated from ds/ (Wave 2 — consolidação Binance)
export { PageSurface, type PageSurfaceProps } from "./PageSurface";
export { StatPill, type StatPillProps, type Intent as StatIntent } from "./StatPill";
export { BaseRow, type BaseRowProps, type RowStatus } from "./BaseRow";
export { CodeBlock, type CodeBlockProps, type CodeLang } from "./CodeBlock";
export { DiffViewer, type DiffViewerProps, type DiffMode } from "./DiffViewer";
export { TreeNode, type TreeNodeProps, type TreeNodeData } from "./TreeNode";
