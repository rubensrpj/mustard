import { createContext, useContext, useState, type ReactNode } from "react";
import type { TimeRange } from "@/lib/types/telemetry";

interface TelemetryTimeRangeContextValue {
  timeRange: TimeRange;
  setTimeRange: (r: TimeRange) => void;
}

const TelemetryTimeRangeContext = createContext<TelemetryTimeRangeContextValue>({
  timeRange: "today",
  setTimeRange: () => undefined,
});

export function TelemetryTimeRangeProvider({ children }: { children: ReactNode }) {
  const [timeRange, setTimeRange] = useState<TimeRange>("today");
  return (
    <TelemetryTimeRangeContext.Provider value={{ timeRange, setTimeRange }}>
      {children}
    </TelemetryTimeRangeContext.Provider>
  );
}

export function useTelemetryTimeRange(): TelemetryTimeRangeContextValue {
  return useContext(TelemetryTimeRangeContext);
}
