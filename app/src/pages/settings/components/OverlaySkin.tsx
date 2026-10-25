import { useState } from "react";
import { Button, Header, Label, Switch } from "@/components";
import { PETS, getPet, setPet, type PetChoice } from "@/lib/pet";
import { PetSprite } from "@/pages/overlay/components/PetSprite";
import { SkinPreviewPicker } from "@/components/setup";
import { type SkinId, getSkin, getTerminalWright, setTerminalWright } from "@/lib/overlay-skin";

/**
 * Pick the overlay's shape.
 *
 * The brief was "have options": the overlay carries a control strip over
 * KiCad, and how much of one it should be depends on the work. The catalogue
 * lives in `@/lib/overlay-skin` so this control, the wizard and the overlay
 * read the same list; `SkinPreviewPicker` draws it here and in the wizard.
 *
 * The choice is stored, not lifted into a context, because the overlay is a
 * separate webview from this one. localStorage plus a `storage` event is how
 * the two windows already talk (see `WRIGHT_STORAGE_KEYS.LAST_RUN`), so a skin
 * picked here reaches the overlay without a new bridge.
 */
export const OverlaySkin = ({ className }: { className?: string }) => {
  const [skin, setSkinState] = useState<SkinId>(() => getSkin());
  const [wrightInTerminal, setWrightInTerminal] = useState(() => getTerminalWright());
  const [pet, setPetState] = useState<PetChoice>(() => getPet());

  const choosePet = (choice: PetChoice) => {
    setPet(choice);
    setPetState(choice);
  };

  const toggleWright = (enabled: boolean) => {
    setTerminalWright(enabled);
    setWrightInTerminal(enabled);
  };

  return (
    <div id="overlay-skin" className={`space-y-3 ${className ?? ""}`} data-testid="overlay-skin-settings">
      <Header
        isMainTitle
        title="Overlay"
        description="How much the strip shows. The desktop is for intensive work; this is meant to stay out of the way."
      />
      <SkinPreviewPicker onChange={setSkinState} />

      {/* Only meaningful for the one skin that has a shell in it. */}
      {skin === "terminal" ? (
        <div className="flex items-center justify-between gap-3 rounded-md border px-3 py-2">
          <div className="min-w-0">
            <Label className="text-sm font-medium">Ask Wright from the terminal</Label>
            <p className="mt-1 text-xs text-muted-foreground">
              A line starting with a Capital letter goes to Wright; <code>!</code> asks it to
              work on it. A leading space always runs in the shell. Off makes this an
              ordinary terminal.
            </p>
          </div>
          <Switch
            checked={wrightInTerminal}
            onCheckedChange={toggleWright}
            data-testid="terminal-wright-switch"
            aria-label={wrightInTerminal ? "Turn off Wright routing" : "Turn on Wright routing"}
          />
        </div>
      ) : null}

      {/* What the bar folds down to (the left arrow). A pet in the manner of
          the Codex app's: hover it for Speak and Write, click it to unfold.
          Only the plain Bar skin folds this way. */}
      {skin === "plain" ? (
        <div className="rounded-md border px-3 py-2" data-testid="overlay-pet-settings">
          <Label className="text-sm font-medium">Folded pet</Label>
          <p className="mt-1 text-xs text-muted-foreground">
            Fold the bar with its left arrow and it becomes a pet. Hover it to Speak or
            Write; click it to open the bar again. Off keeps the small pill.
          </p>
          <div className="mt-2 flex flex-wrap items-end gap-2" role="radiogroup" aria-label="Folded pet">
            {PETS.map((option) => (
              <Button
                key={option.id}
                variant={pet === option.id ? "secondary" : "ghost"}
                className="h-auto flex-col gap-1 px-2 py-1.5"
                role="radio"
                aria-checked={pet === option.id}
                title={option.credit}
                onClick={() => choosePet(option.id)}
                data-testid={`overlay-pet-${option.id}`}
              >
                <PetSprite pet={option.id} row="idle" width={48} still />
                <span className="text-xs">{option.name}</span>
              </Button>
            ))}
            <Button
              variant={pet === "off" ? "secondary" : "ghost"}
              className="h-auto px-3 py-1.5 text-xs"
              role="radio"
              aria-checked={pet === "off"}
              onClick={() => choosePet("off")}
              data-testid="overlay-pet-off"
            >
              Off (pill)
            </Button>
          </div>
        </div>
      ) : null}
    </div>
  );
};
