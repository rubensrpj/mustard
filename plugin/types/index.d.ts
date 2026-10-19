// The contract this plugin ships: where the person is in the Mustard panel,
// kept in the host's state so a reload of the module keeps the screen.
export type MustardPanelScreen = 'overview' | 'spec' | 'items' | 'item' | 'usage' | 'settings';
export type MustardPanelKind = 'task' | 'criterion' | 'decision' | 'rule' | 'question';
export type MustardPanel = {
  screen: MustardPanelScreen;
  spec: string | null;
  kind: MustardPanelKind;
  query: string;
  filter: 'all' | 'attention' | 'committed' | 'uncommitted';
  rows: number;
  item: string | null;
  back: { screen: MustardPanelScreen; item: string | null; kind: MustardPanelKind }[];
  history: boolean;
  searching: boolean;
  pickSpec: boolean;
  delivered: boolean;
};
declare module 'claude-code' {
  interface PluginState {
    mustard: { panel: MustardPanel };
  }
}
