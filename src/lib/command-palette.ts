export type CommandPaletteCommand = {
  id: string;
  title: string;
  description?: string;
  group?: string;
  keywords?: string[];
  shortcut?: string;
  disabled?: boolean;
  run: () => void | Promise<void>;
};
