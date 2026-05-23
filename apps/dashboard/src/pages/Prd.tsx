import { useState, useEffect, useMemo, useDeferredValue, useRef } from 'react';
import { useQuery } from '@tanstack/react-query';
import { Plus, X } from 'lucide-react';
import { toast } from 'sonner';
import { Markdown } from '@/components/page/Markdown';
import { generatePrdMarkdown, slugify } from '@/lib/prd-template';
import { useStore } from '@/lib/store';
import { discoverProjects } from '@/api/discovery';
import { CollapsibleGroup } from '@/components/page';
import { IntentHero } from '@/features/prd/IntentHero';
import { EntityPicker } from '@/features/prd/EntityPicker';
import { EditableList } from '@/features/prd/EditableList';
import { useEntityRegistry } from '@/hooks/useEntityRegistry';
import { useLapidator } from '@/hooks/useLapidator';
import type { LapidatedPrd } from '@/lib/types/prd';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';

const DRAFT_KEY = 'mustard-prd-draft';

interface AcItem {
  title: string;
  command: string;
}

interface PrdForm {
  type: 'feature' | 'bugfix';
  title: string;
  scope: 'light' | 'full';
  projectId: string;
  summary: string;
  why: string;
  layers: {
    backend: boolean;
    frontend: boolean;
    database: boolean;
    design: boolean;
    docs: boolean;
    testes: boolean;
  };
  boundaries: string;
  checklist: string;
  acceptanceCriteria: AcItem[];
  decisionsNotObvious: string;
  nonGoals: string;
}

const DEFAULT_FORM: PrdForm = {
  type: 'feature',
  title: '',
  scope: 'light',
  projectId: '',
  summary: '',
  why: '',
  layers: { backend: false, frontend: false, database: false, design: false, docs: false, testes: false },
  boundaries: '',
  checklist: '',
  acceptanceCriteria: [{ title: '', command: '' }],
  decisionsNotObvious: '',
  nonGoals: '',
};

const LAYER_SUGGESTIONS: Record<string, { boundaries: string; checklist: string }> = {
  backend: { boundaries: 'Endpoints in api/v1/...', checklist: 'Add endpoint' },
  frontend: { boundaries: 'UI components in src/components/...', checklist: 'Add UI component' },
  database: { boundaries: 'Schema changes in migrations/...', checklist: 'Add migration' },
  design: { boundaries: 'Design tokens in src/styles/...', checklist: 'Update design tokens' },
  docs: { boundaries: 'Documentation in docs/...', checklist: 'Update documentation' },
  testes: { boundaries: 'Tests in __tests__/...', checklist: 'Add tests' },
};

const inputClass = 'w-full bg-card border border-border rounded text-sm px-2 py-1.5 focus:border-primary outline-none';
const invalidInputClass = 'w-full bg-card border border-destructive rounded text-sm px-2 py-1.5 focus:border-primary outline-none';
const labelClass = 'text-[13px] font-medium';

function splitLines(text: string): string[] {
  return text.split('\n').map((l) => l.trim()).filter(Boolean);
}

// Like splitLines but preserves blanks and whitespace so EditableList rows
// stay editable mid-typing. Only used to round-trip through the textarea-
// backed `form.boundaries` / `form.checklist` strings.
function splitLinesRaw(text: string): string[] {
  if (text === '') return [];
  return text.split('\n');
}

function mergeLineNonDestructive(existing: string, line: string): string {
  const lines = existing.split('\n');
  if (lines.some((l) => l.trim() === line)) return existing;
  return existing ? `${existing}\n${line}` : line;
}

export function Prd() {
  const [form, setForm] = useState<PrdForm>(DEFAULT_FORM);
  const [errors, setErrors] = useState<Record<string, boolean>>({});
  const saveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Wave 3 — lapidator state (kept local; not persisted to draft).
  const lap = useLapidator();
  const [aiSuggestedEntities, setAiSuggestedEntities] = useState<string[]>([]);
  // false = auto-derived from AI (default). true = user opted into manual override.
  const [scopeOverride, setScopeOverride] = useState(false);

  const projectsRoot = useStore((s) => s.projectsRoot);
  const { data: projects } = useQuery({
    queryKey: ['discover', projectsRoot],
    queryFn: () => discoverProjects(projectsRoot!),
    enabled: !!projectsRoot,
    staleTime: 60_000,
  });

  const activeProjectPath = useMemo(() => {
    if (!form.projectId || !projects) return null;
    return projects.find((p) => p.id === form.projectId)?.path ?? null;
  }, [form.projectId, projects]);

  const { data: entities = [] } = useEntityRegistry(activeProjectPath);

  useEffect(() => {
    try {
      const raw = localStorage.getItem(DRAFT_KEY);
      if (!raw) return;
      const parsed = JSON.parse(raw) as PrdForm;
      if (parsed && typeof parsed === 'object' && 'type' in parsed) {
        // slug no longer exists in the form — strip it silently on legacy drafts.
        const legacy = parsed as PrdForm & { slug?: string };
        const { slug: _slug, ...rest } = legacy;
        void _slug;
        setForm({ ...DEFAULT_FORM, ...rest });
      }
    } catch {
      // ignore corrupt draft
    }
  }, []);

  const deferredForm = useDeferredValue(form);

  useEffect(() => {
    if (saveTimer.current) clearTimeout(saveTimer.current);
    saveTimer.current = setTimeout(() => {
      localStorage.setItem(DRAFT_KEY, JSON.stringify(form));
    }, 500);
    return () => {
      if (saveTimer.current) clearTimeout(saveTimer.current);
    };
  }, [form]);

  // Slug is always derived from title — no manual input.
  const slugDerived = useMemo(() => slugify(form.title), [form.title]);

  const markdownPreview = useMemo(() => {
    try {
      return generatePrdMarkdown({
        type: deferredForm.type,
        slug: slugDerived,
        title: deferredForm.title,
        summary: deferredForm.summary,
        why: deferredForm.why || undefined,
        scope: deferredForm.scope,
        boundaries: splitLines(deferredForm.boundaries),
        checklist: splitLines(deferredForm.checklist),
        acceptanceCriteria: deferredForm.acceptanceCriteria.filter((ac) => ac.title || ac.command),
        decisionsNotObvious: splitLines(deferredForm.decisionsNotObvious) || undefined,
        nonGoals: splitLines(deferredForm.nonGoals) || undefined,
        project: deferredForm.projectId || undefined,
      });
    } catch {
      return '_(preencha os campos obrigatórios)_';
    }
  }, [deferredForm, slugDerived]);

  function setField<K extends keyof PrdForm>(key: K, value: PrdForm[K]) {
    setForm((f) => ({ ...f, [key]: value }));
    if (errors[key]) setErrors((e) => ({ ...e, [key]: false }));
  }

  function toggleLayer(layer: keyof PrdForm['layers'], checked: boolean) {
    setForm((f) => {
      const newLayers = { ...f.layers, [layer]: checked };
      if (checked) {
        const sug = LAYER_SUGGESTIONS[layer];
        return {
          ...f,
          layers: newLayers,
          boundaries: mergeLineNonDestructive(f.boundaries, sug.boundaries),
          checklist: mergeLineNonDestructive(f.checklist, sug.checklist),
        };
      }
      return { ...f, layers: newLayers };
    });
  }

  function addAc() {
    setForm((f) => ({ ...f, acceptanceCriteria: [...f.acceptanceCriteria, { title: '', command: '' }] }));
  }

  function removeAc(index: number) {
    setForm((f) => ({ ...f, acceptanceCriteria: f.acceptanceCriteria.filter((_, i) => i !== index) }));
  }

  function updateAc(index: number, field: keyof AcItem, value: string) {
    setForm((f) => {
      const updated = f.acceptanceCriteria.map((ac, i) => (i === index ? { ...ac, [field]: value } : ac));
      return { ...f, acceptanceCriteria: updated };
    });
  }

  async function handleLapidate() {
    if (!form.projectId) return;
    const project = projects?.find((p) => p.id === form.projectId);
    if (!project?.path) {
      toast.error('Selecione um projeto válido');
      return;
    }
    await lap.lapidate(project.path, (result: LapidatedPrd) => {
      setForm((prev) => ({
        ...prev,
        type: result.type,
        title: result.title || prev.title,
        scope: result.scope,
        summary: result.summary,
        why: result.why ?? prev.why,
        layers: result.layers,
        boundaries: result.boundaries.join('\n'),
        checklist: result.checklist.join('\n'),
        acceptanceCriteria: result.acceptanceCriteria.length
          ? result.acceptanceCriteria
          : prev.acceptanceCriteria,
        decisionsNotObvious: result.decisionsNotObvious?.join('\n') ?? prev.decisionsNotObvious,
        nonGoals: result.nonGoals?.join('\n') ?? prev.nonGoals,
      }));
      setAiSuggestedEntities(result._confront.entitiesFound);
      setScopeOverride(false);
    });
  }

  function validate(): boolean {
    const newErrors: Record<string, boolean> = {};
    if (!form.summary.trim()) newErrors.summary = true;
    if (!splitLines(form.boundaries).length) newErrors.boundaries = true;
    if (!splitLines(form.checklist).length) newErrors.checklist = true;
    setErrors(newErrors);
    if (Object.keys(newErrors).length > 0) {
      toast.error('Preencha os campos obrigatórios');
      return false;
    }
    return true;
  }

  function copyMarkdown() {
    if (!validate()) return;
    navigator.clipboard.writeText(markdownPreview).then(() => toast.success('Copiado!'));
  }

  function copyWithPrefix() {
    if (!validate()) return;
    const prefixed = `/mustard:${form.type} ${slugDerived}\n\n${markdownPreview}`;
    navigator.clipboard.writeText(prefixed).then(() => toast.success('Copiado!'));
  }

  function clearDraft() {
    localStorage.removeItem(DRAFT_KEY);
    setForm(DEFAULT_FORM);
    setErrors({});
    lap.reset();
    setAiSuggestedEntities([]);
    setScopeOverride(false);
  }

  const showScopeButtons = scopeOverride || !lap.confront;

  return (
    <div className="flex flex-col gap-4">
      <div>
        <span className="text-[11px] uppercase tracking-wider text-muted-foreground">Mustard / PRD</span>
        <h1 className="text-base font-medium">PRD Builder</h1>
        <p className="text-[13px] text-muted-foreground">Gere specs no formato Mustard.</p>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
        {/* Form column */}
        <div className="flex flex-col gap-3">
          {/* Intent (always visible — the new entry point) */}
          <div className="flex flex-col gap-1">
            <label className={labelClass}>Intenção livre</label>
            <IntentHero
              intent={lap.intent}
              onIntentChange={lap.setIntent}
              onLapidate={handleLapidate}
              isLapidating={lap.isLapidating}
              claudeAvailable={lap.claudeAvailable}
              lapidateError={lap.lapidateError}
              confront={lap.confront}
              projectSelected={!!form.projectId}
            />
          </div>

          {/* Identity */}
          <CollapsibleGroup label="Identidade" defaultOpen hint="Tipo, título e projeto.">
            <div className="flex flex-col gap-3 pl-5">
              <div className="flex flex-col gap-1">
                <label className={labelClass}>Tipo</label>
                <div className="flex gap-1">
                  {(['feature', 'bugfix'] as const).map((t) => (
                    <button
                      key={t}
                      type="button"
                      onClick={() => setField('type', t)}
                      className={`px-3 py-1.5 rounded text-sm border transition-colors ${form.type === t ? 'bg-primary text-primary-foreground border-primary' : 'border-border text-muted-foreground hover:text-foreground'}`}
                    >
                      {t === 'feature' ? 'Feature' : 'Bugfix'}
                    </button>
                  ))}
                </div>
              </div>
              <div className="flex flex-col gap-1">
                <label htmlFor="prd-title" className={labelClass}>Título</label>
                <input
                  id="prd-title"
                  className={inputClass}
                  value={form.title}
                  placeholder="Ex: dashboard-prd-builder"
                  onChange={(e) => setField('title', e.target.value)}
                />
                {slugDerived && (
                  <span className="text-[11px] text-muted-foreground">
                    Slug: <code className="font-mono">{slugDerived}</code>
                  </span>
                )}
              </div>
              {projects && projects.length > 0 && (
                <div className="flex flex-col gap-1">
                  <label htmlFor="prd-project" className={labelClass}>Projeto</label>
                  <select
                    id="prd-project"
                    className={inputClass}
                    value={form.projectId}
                    onChange={(e) => setField('projectId', e.target.value)}
                  >
                    <option value="">— nenhum —</option>
                    {projects.map((p) => (
                      <option key={p.id} value={p.id}>{p.name}</option>
                    ))}
                  </select>
                </div>
              )}
              {/* Scope: auto-from-AI chip, toggleable to manual buttons. */}
              <div className="flex flex-col gap-1">
                <label className={labelClass}>Escopo</label>
                {!showScopeButtons ? (
                  <div className="flex items-center gap-2">
                    <Badge variant="tag-purple">
                      {form.scope === 'light' ? 'Light (auto)' : 'Full (auto)'}
                    </Badge>
                    <button
                      type="button"
                      onClick={() => setScopeOverride(true)}
                      className="text-[11px] text-muted-foreground hover:text-foreground underline-offset-2 hover:underline"
                    >
                      ajustar
                    </button>
                  </div>
                ) : (
                  <div className="flex gap-1">
                    {(['light', 'full'] as const).map((s) => (
                      <button
                        key={s}
                        type="button"
                        onClick={() => setField('scope', s)}
                        className={`px-3 py-1.5 rounded text-sm border transition-colors ${form.scope === s ? 'bg-primary text-primary-foreground border-primary' : 'border-border text-muted-foreground hover:text-foreground'}`}
                      >
                        {s === 'light' ? 'Light' : 'Full'}
                      </button>
                    ))}
                  </div>
                )}
              </div>
            </div>
          </CollapsibleGroup>

          {/* Details */}
          <CollapsibleGroup label="Detalhes" defaultOpen hint="Resumo, motivação, layers afetados.">
            <div className="flex flex-col gap-3 pl-5">
              <div className="flex flex-col gap-1">
                <label htmlFor="prd-summary" className={labelClass}>Resumo *</label>
                <textarea
                  id="prd-summary"
                  rows={3}
                  className={errors.summary ? invalidInputClass : inputClass}
                  value={form.summary}
                  placeholder="O que será feito e por quê."
                  onChange={(e) => setField('summary', e.target.value)}
                />
              </div>
              <div className="flex flex-col gap-1">
                <label htmlFor="prd-why" className={labelClass}>Por quê? (opcional)</label>
                <textarea
                  id="prd-why"
                  rows={2}
                  className={inputClass}
                  value={form.why}
                  placeholder="Motivação de negócio ou técnica."
                  onChange={(e) => setField('why', e.target.value)}
                />
              </div>
              <fieldset className="flex flex-col gap-1">
                <legend className={`${labelClass} mb-1`}>Layers</legend>
                <div className="flex flex-wrap gap-x-4 gap-y-1">
                  {(Object.keys(form.layers) as Array<keyof PrdForm['layers']>).map((layer) => (
                    <label key={layer} className="flex items-center gap-1.5 text-sm cursor-pointer">
                      <input
                        type="checkbox"
                        checked={form.layers[layer]}
                        onChange={(e) => toggleLayer(layer, e.target.checked)}
                        className="rounded"
                      />
                      {layer.charAt(0).toUpperCase() + layer.slice(1)}
                    </label>
                  ))}
                </div>
              </fieldset>
            </div>
          </CollapsibleGroup>

          {/* Scope of change */}
          <CollapsibleGroup label="Escopo de Mudança" defaultOpen hint="Entidades e paths afetados.">
            <div className="flex flex-col gap-3 pl-5">
              <div className="flex flex-col gap-1">
                <label className={labelClass}>Entidades</label>
                <EntityPicker
                  entities={entities}
                  selected={lap.selectedEntities}
                  onChange={lap.setSelectedEntities}
                  prePicked={aiSuggestedEntities}
                />
              </div>
              <div className="flex flex-col gap-1">
                <label className={labelClass}>Boundaries * (1 por linha)</label>
                <EditableList
                  items={splitLinesRaw(form.boundaries)}
                  onChange={(items) => setField('boundaries', items.join('\n'))}
                  placeholder="src/lib/prd-template.ts"
                  itemAriaPrefix="Boundary"
                />
                {errors.boundaries && (
                  <span className="text-[11px] text-destructive">Adicione pelo menos um boundary.</span>
                )}
              </div>
            </div>
          </CollapsibleGroup>

          {/* Plan */}
          <CollapsibleGroup label="Plano" defaultOpen hint="Checklist de execução.">
            <div className="flex flex-col gap-1 pl-5">
              <label className={labelClass}>Checklist * (1 por linha)</label>
              <EditableList
                items={splitLinesRaw(form.checklist)}
                onChange={(items) => setField('checklist', items.join('\n'))}
                placeholder="Criar função pura"
                itemAriaPrefix="Item do checklist"
              />
              {errors.checklist && (
                <span className="text-[11px] text-destructive">Adicione pelo menos um item.</span>
              )}
            </div>
          </CollapsibleGroup>

          {/* Acceptance Criteria — unchanged structure */}
          <CollapsibleGroup label="Critérios" defaultOpen hint="Acceptance criteria runnable.">
            <div className="flex flex-col gap-2 pl-5">
              {form.acceptanceCriteria.map((ac, i) => (
                <div key={i} className="flex flex-col gap-1 border border-border rounded p-2 relative">
                  <div className="flex items-center justify-between gap-2">
                    <span className="text-[11px] uppercase tracking-wider text-muted-foreground">AC-{i + 1}</span>
                    <button
                      type="button"
                      aria-label={`Remover AC-${i + 1}`}
                      onClick={() => removeAc(i)}
                      className="text-muted-foreground hover:text-foreground"
                    >
                      <X className="h-3.5 w-3.5" />
                    </button>
                  </div>
                  <input
                    className={inputClass}
                    placeholder="Título do critério"
                    value={ac.title}
                    onChange={(e) => updateAc(i, 'title', e.target.value)}
                    aria-label={`AC-${i + 1} título`}
                  />
                  <textarea
                    rows={2}
                    className={`${inputClass} font-mono text-xs`}
                    placeholder="npx tsc --noEmit"
                    value={ac.command}
                    onChange={(e) => updateAc(i, 'command', e.target.value)}
                    aria-label={`AC-${i + 1} comando`}
                  />
                </div>
              ))}
              <button
                type="button"
                onClick={addAc}
                aria-label="Adicionar critério de aceite"
                className="flex items-center gap-1.5 text-sm text-muted-foreground hover:text-foreground border border-dashed border-border rounded px-3 py-1.5"
              >
                <Plus className="h-3.5 w-3.5" /> Adicionar AC
              </button>
            </div>
          </CollapsibleGroup>

          {/* Advanced */}
          <CollapsibleGroup label="Avançado" hint="Decisões não-óbvias e não-goals.">
            <div className="flex flex-col gap-3 pl-5">
              <div className="flex flex-col gap-1">
                <label htmlFor="prd-decisions" className={labelClass}>Decisões não-óbvias (opcional)</label>
                <textarea
                  id="prd-decisions"
                  rows={2}
                  className={inputClass}
                  value={form.decisionsNotObvious}
                  placeholder="Uma decisão por linha."
                  onChange={(e) => setField('decisionsNotObvious', e.target.value)}
                />
              </div>
              <div className="flex flex-col gap-1">
                <label htmlFor="prd-nongoals" className={labelClass}>Non-Goals (opcional)</label>
                <textarea
                  id="prd-nongoals"
                  rows={2}
                  className={inputClass}
                  value={form.nonGoals}
                  placeholder="Um por linha."
                  onChange={(e) => setField('nonGoals', e.target.value)}
                />
              </div>
            </div>
          </CollapsibleGroup>
        </div>

        {/* Preview */}
        <div className="flex flex-col gap-2">
          <div className="text-[11px] uppercase tracking-wider text-muted-foreground">Preview</div>
          <div
            className="border border-border rounded p-4 bg-card overflow-y-auto"
            style={{ maxHeight: 'calc(100vh - 200px)' }}
          >
            <Markdown content={markdownPreview} />
          </div>
        </div>
      </div>

      {/* Actions */}
      <div className="flex items-center gap-2 pt-2 border-t border-border">
        <Button type="button" onClick={copyMarkdown} size="sm">
          Copiar markdown
        </Button>
        <Button type="button" onClick={copyWithPrefix} size="sm">
          Copiar com /mustard:{form.type}
        </Button>
        <Button
          type="button"
          onClick={clearDraft}
          variant="outline"
          size="sm"
          className="ml-auto"
        >
          Limpar
        </Button>
      </div>
    </div>
  );
}
