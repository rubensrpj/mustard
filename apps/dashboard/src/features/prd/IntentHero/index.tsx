import { Loader2, Wand2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Textarea } from '@/components/ui/textarea';
import { Tooltip, TooltipContent, TooltipTrigger, TooltipProvider } from '@/components/ui/tooltip';
import { ConfrontBanner } from '@/components/prd/ConfrontBanner';
import type { PrdConfront } from '@/lib/types/prd';

/**
 * Hero block at the top of the PRD page. Captures a free-form "intent" and
 * exposes a single "Lapidar com IA" call-to-action that runs the Claude CLI
 * lapidator. Renders banners for:
 *   - confront warnings (entitiesMissing / pathsMissing) from a successful run
 *   - error string from a failed run
 *   - missing-CLI hint when `claudeAvailable === false`
 *
 * Disabled rule for the lapidate button:
 *   !intent.trim() || !projectSelected || isLapidating || claudeAvailable === false
 */
export interface IntentHeroProps {
  intent: string;
  onIntentChange: (next: string) => void;
  onLapidate: () => Promise<void>;
  isLapidating: boolean;
  claudeAvailable: boolean | null;
  lapidateError: string | null;
  confront: PrdConfront | null;
  projectSelected: boolean;
}

export function IntentHero({
  intent,
  onIntentChange,
  onLapidate,
  isLapidating,
  claudeAvailable,
  lapidateError,
  confront,
  projectSelected,
}: IntentHeroProps) {
  const claudeMissing = claudeAvailable === false;
  const disabled = !intent.trim() || !projectSelected || isLapidating || claudeMissing;

  const button = (
    <Button
      type="button"
      onClick={() => void onLapidate()}
      disabled={disabled}
      aria-busy={isLapidating}
      variant="default"
      size="sm"
    >
      {isLapidating ? <Loader2 className="animate-spin" /> : <Wand2 />}
      {isLapidating ? 'Lapidando…' : 'Lapidar com IA'}
    </Button>
  );

  return (
    <div className="flex flex-col gap-2">
      <Textarea
        rows={4}
        value={intent}
        placeholder="Descreva em uma ou duas frases o que você quer construir (a IA vai estruturar o resto)."
        onChange={(e) => onIntentChange(e.target.value)}
        aria-label="Intenção livre"
      />
      <div className="flex items-center gap-2 flex-wrap">
        {claudeMissing ? (
          <TooltipProvider>
            <Tooltip>
              <TooltipTrigger asChild>
                <span>{button}</span>
              </TooltipTrigger>
              <TooltipContent>
                Claude CLI não encontrado — instale em claude.ai/cli
              </TooltipContent>
            </Tooltip>
          </TooltipProvider>
        ) : (
          button
        )}
        {!projectSelected && (
          <span className="text-xs text-muted-foreground">
            Selecione um projeto para habilitar a lapidação.
          </span>
        )}
      </div>

      <ConfrontBanner confront={confront} />

      {lapidateError && (
        <div
          role="alert"
          className="text-xs text-destructive border border-destructive/40 bg-destructive/5 rounded px-3 py-2"
        >
          {lapidateError}
        </div>
      )}
    </div>
  );
}
