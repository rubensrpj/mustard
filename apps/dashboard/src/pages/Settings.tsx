import { useState, useMemo } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { toast } from 'sonner';
import { useTranslation } from 'react-i18next';
import { useStore } from '@/lib/store';
import { discoverProjects } from '@/api/discovery';
import { readEnv, writeEnv } from '@/api/env';
import { ENV_CATALOG, type EnvKey } from '@/data/env-catalog';
import {
  readSettings,
  setLanguage,
  setTone,
  type ProjectSettings,
} from '@/lib/dashboard';
import { Card, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Badge } from '@/components/ui/badge';
import {
  PageSurface,
  EditorialBand,
  DataCard,
  CollapsibleGroup,
  EmptyState,
  SectionHeader,
} from '@/components/page';

function omitKey(obj: Record<string, string>, key: string): Record<string, string> {
  const next = { ...obj };
  delete next[key];
  return next;
}

/**
 * One environment-variable row. The human-readable `label` is the primary
 * heading; the raw env var name sits below as a monospace subtitle so it is
 * still discoverable without dominating the form.
 */
function EnvField({
  envKey: k,
  value,
  pending,
  onChange,
}: {
  envKey: EnvKey;
  value: string;
  pending: boolean;
  onChange: (key: string, value: string) => void;
}) {
  const inputId = `env-${k.key}`;
  return (
    <div className="px-4 pb-3 pt-1 flex flex-col gap-1">
      <div className="flex items-baseline gap-2 flex-wrap">
        <label htmlFor={inputId} className="text-[13px] font-medium text-foreground">
          {k.label}
        </label>
        {pending && (
          <Badge variant="outline" className="text-[10px] border-[--color-accent-mustard]/40 text-[--color-accent-mustard]">
            alteração pendente
          </Badge>
        )}
      </div>
      <code className="font-mono text-[11px] text-muted-foreground">{k.key}</code>
      <p className="text-[12.5px] text-muted-foreground leading-relaxed">{k.desc}</p>
      {k.options.length === 0 ? (
        <input
          id={inputId}
          className="bg-card border border-border rounded-md text-sm px-2 py-1 focus:border-primary outline-none w-full transition-colors"
          value={value}
          onChange={(e) => onChange(k.key, e.target.value)}
          placeholder={k.default || 'vazio'}
        />
      ) : (
        <select
          id={inputId}
          className="bg-card border border-border rounded-md text-sm px-2 py-1 focus:border-primary outline-none w-full transition-colors"
          value={value}
          onChange={(e) => onChange(k.key, e.target.value)}
        >
          {k.options.map((opt) => (
            <option key={opt} value={opt}>
              {opt === '' ? '(vazio)' : opt}
              {opt === k.default ? ' — padrão' : ''}
            </option>
          ))}
        </select>
      )}
      {k.valueDocs[value] && (
        <p className="text-[12px] text-muted-foreground mt-0.5">
          {k.valueDocs[value]}
        </p>
      )}
    </div>
  );
}

/**
 * Dashboard-wide UI language toggle — moved here from the removed
 * `/preferences` page. Global: independent of the selected project, so it
 * renders even when no project is picked. Backed by the zustand `language`
 * slice, which drives both i18n surfaces (`src/i18n.ts` + `src/lib/i18n.ts`).
 */
function DashboardLanguageCard() {
  const { t } = useTranslation();
  const language = useStore((s) => s.language);
  const setUiLanguage = useStore((s) => s.setLanguage);

  return (
    <DataCard padded>
      <SectionHeader
        title={t('preferences.language')}
        description={t('preferences.description')}
      />
      <div className="flex items-center gap-2 pt-3">
        {(['pt-BR', 'en-US'] as const).map((lng) => (
          <button
            key={lng}
            onClick={() => setUiLanguage(lng)}
            className={language === lng
              ? "bg-primary text-primary-foreground px-3 py-1.5 rounded text-sm"
              : "text-muted-foreground hover:text-foreground px-3 py-1.5 rounded text-sm border border-border"}
          >
            {lng === 'pt-BR' ? t('preferences.languagePt') : t('preferences.languageEn')}
          </button>
        ))}
      </div>
    </DataCard>
  );
}

/**
 * Wave 4 mustard-unification — per-project language + tone selector.
 *
 * Writes are routed through `commands::settings::{set_language,set_tone}` so
 * the BCP-47 / tone validation lives on the Rust side. The UI keeps two
 * native `<select>` controls (matching the `EnvField` style) — no fancy combo
 * box — because the catalog is small and stable.
 */
function LanguageAndToneCard({ repoPath }: { repoPath: string }) {
  const qc = useQueryClient();
  const { data } = useQuery<ProjectSettings>({
    queryKey: ['settings', repoPath],
    queryFn: () => readSettings(repoPath),
    staleTime: 60_000,
  });

  const currentLang = data?.lang ?? 'pt-BR';
  const currentTone = data?.tone ?? 'didactic';

  const langMutation = useMutation({
    mutationFn: (lang: string) => setLanguage(repoPath, lang),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['settings', repoPath] });
      toast.success('Idioma atualizado');
    },
    onError: (e: Error) => toast.error('Erro: ' + e.message),
  });

  const toneMutation = useMutation({
    mutationFn: (tone: string) => setTone(repoPath, tone),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['settings', repoPath] });
      toast.success('Tom atualizado');
    },
    onError: (e: Error) => toast.error('Erro: ' + e.message),
  });

  return (
    <DataCard>
      <Card size="sm" className="border-none bg-transparent">
        <CardHeader>
          <CardTitle className="text-sm font-medium">Idioma e tom (locale)</CardTitle>
          <CardDescription className="text-[13px] text-muted-foreground">
            Define o idioma dos banners e o tom de voz em `mustard.json`. Valores
            BCP-47: `pt-BR` ou `en-US`.
          </CardDescription>
        </CardHeader>
        <div className="px-4 pb-3 pt-1 flex flex-col gap-1">
          <label htmlFor="settings-lang" className="text-[13px] font-medium text-foreground">
            Idioma (lang / locale)
          </label>
          <code className="font-mono text-[11px] text-muted-foreground">mustard.json#lang</code>
          <select
            id="settings-lang"
            className="bg-card border border-border rounded-md text-sm px-2 py-1 focus:border-primary outline-none w-full transition-colors"
            value={currentLang}
            disabled={langMutation.isPending}
            onChange={(e) => langMutation.mutate(e.target.value)}
          >
            <option value="pt-BR">pt-BR — Portugues do Brasil</option>
            <option value="en-US">en-US — English (United States)</option>
          </select>
        </div>
        <div className="px-4 pb-3 pt-1 flex flex-col gap-1">
          <label htmlFor="settings-tone" className="text-[13px] font-medium text-foreground">
            Tom (tone)
          </label>
          <code className="font-mono text-[11px] text-muted-foreground">mustard.json#tone</code>
          <select
            id="settings-tone"
            className="bg-card border border-border rounded-md text-sm px-2 py-1 focus:border-primary outline-none w-full transition-colors"
            value={currentTone}
            disabled={toneMutation.isPending}
            onChange={(e) => toneMutation.mutate(e.target.value)}
          >
            <option value="didactic">didactic — didatico (expande siglas)</option>
            <option value="technical">technical — tecnico (mantem jargao)</option>
            <option value="concise">concise — conciso (sem parenteticos)</option>
          </select>
        </div>
      </Card>
    </DataCard>
  );
}

export function Settings() {
  const { t } = useTranslation();
  const projectsRoot = useStore((s) => s.projectsRoot);
  const selectedProjectId = useStore((s) => s.selectedProjectId);

  const { data: projects } = useQuery({
    queryKey: ['discover', projectsRoot],
    queryFn: () => discoverProjects(projectsRoot!),
    enabled: !!projectsRoot,
    staleTime: 60_000,
  });

  const selectedProject = useMemo(
    () => projects?.find((p) => p.id === selectedProjectId) ?? null,
    [projects, selectedProjectId],
  );

  const queryClient = useQueryClient();
  const [pendingEnv, setPendingEnv] = useState<Record<string, string>>({});

  const { data: envFromDisk } = useQuery({
    queryKey: ['env', selectedProject?.path],
    queryFn: () => readEnv(selectedProject!.path),
    enabled: !!selectedProject,
    staleTime: 60_000,
  });

  const effectiveEnv = useMemo(
    () => ({ ...envFromDisk, ...pendingEnv }),
    [envFromDisk, pendingEnv],
  );

  const hasPending = useMemo(() => Object.keys(pendingEnv).length > 0, [pendingEnv]);

  const saveMutation = useMutation({
    mutationFn: (env: Record<string, string>) => writeEnv(selectedProject!.path, env),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['env', selectedProject?.path] });
      setPendingEnv({});
      toast.success('Salvo');
    },
    onError: (e: Error) => toast.error('Erro: ' + e.message),
  });

  function onSelectChange(key: string, value: string) {
    setPendingEnv((prev) =>
      value === (envFromDisk ?? {})[key]
        ? omitKey(prev, key)
        : { ...prev, [key]: value },
    );
  }

  function onSave() {
    saveMutation.mutate(effectiveEnv);
  }

  function onDiscard() {
    setPendingEnv({});
  }

  return (
    <PageSurface>
      <EditorialBand
        eyebrow="Mustard"
        title={t('settings.title')}
        subtitle={
          <>
            {t('settings.envDescriptionBefore')}
            <code className="font-mono">.claude/settings.json#env</code>
            {t('settings.envDescriptionAfter')}
          </>
        }
      />

      <DashboardLanguageCard />

      {!selectedProject ? (
        <EmptyState
          title={t('settings.envSelectProject')}
          description={t('settings.envSelectProject')}
        />
      ) : (
        <>
          <LanguageAndToneCard repoPath={selectedProject.path} />
          <div className="flex flex-col gap-1">
            <h2 className="text-sm font-medium">{t('settings.envTitle')} — {selectedProject.name}</h2>
            <p className="text-[13px] text-muted-foreground">
              {t('settings.envHelpTextBefore')}<code className="font-mono">MUSTARD_*</code>{t('settings.envHelpTextSep')}<code className="font-mono">OTEL_*</code>{t('settings.envHelpTextSep')}<code className="font-mono">CLAUDE_CODE_*</code>{t('settings.envHelpTextAfter')}<code className="font-mono">.claude/settings.json#env</code>{t('settings.envHelpTextEnd')}
            </p>
          </div>
          {ENV_CATALOG.map((group) => {
            const basicKeys = group.keys.filter((k) => !k.advanced);
            const advancedKeys = group.keys.filter((k) => k.advanced);
            return (
              <DataCard key={group.group}>
                <Card size="sm" className="border-none bg-transparent">
                  <CardHeader>
                    <CardTitle className="text-sm font-medium">{group.group}</CardTitle>
                    <CardDescription className="text-[13px] text-muted-foreground">
                      {group.desc}
                    </CardDescription>
                  </CardHeader>
                  {basicKeys.map((k) => (
                    <EnvField
                      key={k.key}
                      envKey={k}
                      value={effectiveEnv[k.key] ?? k.default}
                      pending={k.key in pendingEnv}
                      onChange={onSelectChange}
                    />
                  ))}
                  {advancedKeys.length > 0 && (
                    <div className="px-4 pb-3">
                      <CollapsibleGroup
                        label="Avançado"
                        count={advancedKeys.length}
                        hint="Knobs de baixo nível (porta, protocolo, transporte). A maioria dos usuários nunca precisa mexer."
                      >
                        <div className="flex flex-col mt-1">
                          {advancedKeys.map((k) => (
                            <EnvField
                              key={k.key}
                              envKey={k}
                              value={effectiveEnv[k.key] ?? k.default}
                              pending={k.key in pendingEnv}
                              onChange={onSelectChange}
                            />
                          ))}
                        </div>
                      </CollapsibleGroup>
                    </div>
                  )}
                </Card>
              </DataCard>
            );
          })}
          <div className="flex items-center gap-2 pt-2 border-t border-border">
            <button
              disabled={!hasPending}
              onClick={onSave}
              className="bg-primary text-primary-foreground px-3 py-1.5 rounded text-sm disabled:opacity-40"
            >
              {t('settings.saveChanges')}
            </button>
            <button
              disabled={!hasPending}
              onClick={onDiscard}
              className="text-muted-foreground hover:text-foreground px-3 py-1.5 rounded text-sm border border-border disabled:opacity-40"
            >
              {t('settings.discardChanges')}
            </button>
            <span className="text-[13px] text-muted-foreground ml-auto">
              {Object.keys(pendingEnv).length} {t('settings.envPending')}
            </span>
          </div>
        </>
      )}
    </PageSurface>
  );
}
