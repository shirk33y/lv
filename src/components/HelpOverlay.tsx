import { showHelp } from "../store";

const BINDINGS: [string, string][] = [
  ["j / k", "navigate down / up"],
  ["h / l", "previous / next directory"],
  ["u", "random file"],
  ["n", "newest file"],
  ["y", "toggle like"],
  ["m", "random favorite"],
  ["b", "latest favorite"],
  ["f", "toggle fullscreen"],
  ["i", "info sidebar"],
  ["x", "log sidebar"],
  ["r", "rescan watched dirs"],
  ["?", "this help"],
  ["q", "quit"],
];

export function HelpOverlay() {
  if (!showHelp.value) return null;

  return (
    <div class="help-overlay" onClick={() => { showHelp.value = false; }}>
      <div class="help-content" onClick={(e) => e.stopPropagation()}>
        <div class="help-title">Keybindings</div>
        <table class="help-table">
          <tbody>
            {BINDINGS.map(([key, desc]) => (
              <tr key={key}>
                <td class="help-key">{key}</td>
                <td class="help-desc">{desc}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
