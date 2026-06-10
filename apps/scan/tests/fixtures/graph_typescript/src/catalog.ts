export interface Shape {
  kind: string;
  area(): number;
}

export type ShapeId = string;

export enum Color {
  Red,
  Green = "green",
}

export const DEFAULT_COLOR = Color.Red;

export function describe(shape: Shape): string {
  return shape.kind;
}
