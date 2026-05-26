import { useTranslation } from 'react-i18next';
import { useStore } from '@/lib/store';
import { PageSurface, EditorialBand, DataCard, SectionHeader } from '@/components/page';

// TODO: future preferences (theme, telemetry opt-in) entram aqui.
export function Preferences() {
  const { t } = useTranslation();
  const language = useStore((s) => s.language);
  const setLanguage = useStore((s) => s.setLanguage);

  return (
    <PageSurface>
      <EditorialBand
        eyebrow="Mustard"
        title={t('preferences.language')}
        subtitle={t('preferences.description')}
      />

      <DataCard padded>
        <SectionHeader
          title={t('preferences.language')}
          description={t('preferences.description')}
        />
        <div className="flex items-center gap-2 pt-3">
          {(['pt-BR', 'en-US'] as const).map((lng) => (
            <button
              key={lng}
              onClick={() => setLanguage(lng)}
              className={language === lng
                ? "bg-primary text-primary-foreground px-3 py-1.5 rounded text-sm"
                : "text-muted-foreground hover:text-foreground px-3 py-1.5 rounded text-sm border border-border"}
            >
              {lng === 'pt-BR' ? t('preferences.languagePt') : t('preferences.languageEn')}
            </button>
          ))}
        </div>
      </DataCard>
    </PageSurface>
  );
}
