import { useState, useEffect, useMemo, useDeferredValue, useRef } from 'react';
import { useQuery } from '@tanstack/react-query';
import { Plus, X } from 'lucide-react';
import { toast } from 'sonner';
import { Markdown } from '@/components/Markdown';
import { generatePrdMarkdown, slugify } from '@/lib/prd-template';
import { useStore } from '@/lib/store';
import { discoverProjects } from '@/api/discovery';

const DRAFT_KEY = 'mustard-prd-draft';

interface AcItem {
  title: string;
  command: string;
}

interface PrdForm {
  type: 'feature' | 'bugfix';
  slug: string;
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
  slug: '',
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

function mergeLineNonDestructive(existing: string, line: string): string {
  const lines = existing.split('\n');
  if (lines.some((l) => l.trim() === line)) return existing;
  return existing ? `${existing}\n${line}` : line;
}

export function Prd() {
  const [form, setForm] = useState<PrdForm>(DEFAULT_FORM);
  const [errors, setErrors] = useState<Record<string, boolean>>({});
  const saveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const projectsRoot = useStore((s) => s.projectsRoot);
  const { data: projects } = useQuery({
    queryKey: ['discover', projectsRoot],
    queryFn: () => discoverProjects(projectsRoot!),
    enabled: !!projectsRoot,
    staleTime: 60_000,
  });

  useEffect(() => {
    try {
      const raw = localStorage.getItem(DRAFT_KEY);
      if (!raw) return;
      const parsed = JSON.parse(raw) as PrdForm;
      if (parsed && typeof parsed === 'object' && 'type' in parsed) {
        setForm(parsed);
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

  const slugDerived = useMemo(
    () => form.slug.trim() || slugify(form.title),
    [form.slug, form.title],
  );

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
  }

  return (
    <div className="flex flex-col gap-4">
      <div>
        <span className="text-[11px] uppercase tracking-wider text-muted-foreground">Mustard / PRD</span>
        <h1 className="text-base font-medium">PRD Builder</h1>
        <p className="text-[13px] text-muted-foreground">Gere specs no formato Mustard.</p>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
        {/* Form */}
        <div className="flex flex-col gap-3">
          {/* Type */}
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

          {/* Slug */}
          <div className="flex flex-col gap-1">
            <label htmlFor="prd-slug" className={labelClass}>Slug</label>
            <input
              id="prd-slug"
              className={inputClass}
              value={form.slug}
              placeholder={slugDerived || 'auto-gerado do título'}
              onChange={(e) => setField('slug', e.target.value)}
            />
          </div>

          {/* Title */}
          <div className="flex flex-col gap-1">
            <label htmlFor="prd-title" className={labelClass}>Título</label>
            <input
              id="prd-title"
              className={inputClass}
              value={form.title}
              placeholder="Ex: dashboard-prd-builder"
              onChange={(e) => setField('title', e.target.value)}
            />
          </div>

          {/* Scope */}
          <div className="flex flex-col gap-1">
            <label className={labelClass}>Escopo</label>
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
          </div>

          {/* Project */}
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

          {/* Summary */}
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

          {/* Why */}
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

          {/* Layer checkboxes */}
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

          {/* Boundaries */}
          <div className="flex flex-col gap-1">
            <label htmlFor="prd-boundaries" className={labelClass}>Boundaries * (1 por linha)</label>
            <textarea
              id="prd-boundaries"
              rows={3}
              className={errors.boundaries ? invalidInputClass : inputClass}
              value={form.boundaries}
              placeholder="src/lib/prd-template.ts&#10;src/pages/Prd.tsx"
              onChange={(e) => setField('boundaries', e.target.value)}
            />
          </div>

          {/* Checklist */}
          <div className="flex flex-col gap-1">
            <label htmlFor="prd-checklist" className={labelClass}>Checklist * (1 por linha)</label>
            <textarea
              id="prd-checklist"
              rows={4}
              className={errors.checklist ? invalidInputClass : inputClass}
              value={form.checklist}
              placeholder="Criar função pura&#10;Criar página Prd&#10;Registrar rota"
              onChange={(e) => setField('checklist', e.target.value)}
            />
          </div>

          {/* Acceptance Criteria */}
          <div className="flex flex-col gap-2">
            <label className={labelClass}>Acceptance Criteria</label>
            <div className="flex flex-col gap-2">
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
            </div>
            <button
              type="button"
              onClick={addAc}
              aria-label="Adicionar critério de aceite"
              className="flex items-center gap-1.5 text-sm text-muted-foreground hover:text-foreground border border-dashed border-border rounded px-3 py-1.5"
            >
              <Plus className="h-3.5 w-3.5" /> Adicionar AC
            </button>
          </div>

          {/* Decisions */}
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

          {/* Non-Goals */}
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
        <button
          type="button"
          onClick={copyMarkdown}
          className="bg-primary text-primary-foreground px-3 py-1.5 rounded text-sm"
        >
          Copiar markdown
        </button>
        <button
          type="button"
          onClick={copyWithPrefix}
          className="bg-primary text-primary-foreground px-3 py-1.5 rounded text-sm"
        >
          Copiar com /mustard:{form.type}
        </button>
        <button
          type="button"
          onClick={clearDraft}
          className="ml-auto text-muted-foreground hover:text-foreground px-3 py-1.5 rounded text-sm border border-border"
        >
          Limpar
        </button>
      </div>
    </div>
  );
}
