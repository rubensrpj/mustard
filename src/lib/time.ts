import dayjs from "dayjs";
import relativeTimePlugin from "dayjs/plugin/relativeTime";
import "dayjs/locale/pt-br";

dayjs.extend(relativeTimePlugin);
dayjs.locale("pt-br");

export function relativeTime(iso: string | null | undefined): string {
  if (!iso) return "";
  try {
    const d = dayjs(iso);
    if (!d.isValid()) return iso;
    return d.fromNow();
  } catch {
    return iso ?? "";
  }
}
