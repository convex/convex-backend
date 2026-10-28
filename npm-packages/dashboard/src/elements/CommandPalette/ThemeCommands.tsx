import { Command } from "cmdk";
import { useTheme } from "next-themes";
import { LaptopIcon, MoonIcon, SunIcon } from "@radix-ui/react-icons";
import { HighlightedText } from "./items";
import { usePaletteAnalytics } from "./analytics";

// The drilled-into "Change Dashboard Theme" page.
export function ThemeCommands({ onClose }: { onClose: () => void }) {
  const { theme: currentTheme, setTheme } = useTheme();
  const { trackSelected } = usePaletteAnalytics();
  const themes = [
    { value: "light", label: "Light", Icon: SunIcon },
    { value: "dark", label: "Dark", Icon: MoonIcon },
    { value: "system", label: "System", Icon: LaptopIcon },
  ];
  return (
    <Command.Group heading="Theme">
      {themes.map(({ value, label, Icon }) => (
        <Command.Item
          key={value}
          value={`theme:${value}`}
          keywords={[label]}
          onSelect={() => {
            trackSelected(`theme:${value}`);
            setTheme(value);
            onClose();
          }}
        >
          <Icon className="text-content-secondary" />
          <HighlightedText text={label} />
          {currentTheme === value && (
            <span className="ml-auto rounded-sm border px-1.5 py-0.5 text-xs text-content-tertiary">
              Current
            </span>
          )}
        </Command.Item>
      ))}
    </Command.Group>
  );
}
