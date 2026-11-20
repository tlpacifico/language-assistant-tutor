/**
 * Preferências locais de áudio do modo artigo: reprodução automática e
 * velocidade da voz do tutor. As alterações aplicam-se à próxima reprodução.
 */

import type { ArticlePreferences } from "../../lib/article-contracts";

const SPEEDS: { value: 8 | 10 | 12; label: string }[] = [
  { value: 8, label: "0.8×" },
  { value: 10, label: "1.0×" },
  { value: 12, label: "1.2×" },
];

export default function ArticlePreferences({
  preferences,
  onAutoplayChange,
  onSpeedChange,
}: {
  preferences: ArticlePreferences;
  onAutoplayChange: (autoplay: boolean) => void;
  onSpeedChange: (speedX10: 8 | 10 | 12) => void;
}) {
  return (
    <fieldset className="article__preferences">
      <legend>Reprodução da voz do tutor</legend>
      <label className="article__preferences-toggle">
        <input
          type="checkbox"
          checked={preferences.autoplay}
          onChange={(event) => onAutoplayChange(event.target.checked)}
        />
        Reproduzir automaticamente
      </label>
      <fieldset className="article__preferences-speed">
        <legend>Velocidade da voz</legend>
        {SPEEDS.map(({ value, label }) => (
          <label key={value}>
            <input
              type="radio"
              name="article-speed"
              value={value}
              checked={preferences.speedX10 === value}
              onChange={() => onSpeedChange(value)}
            />
            {label}
          </label>
        ))}
      </fieldset>
      <p className="practice__hint">
        As alterações aplicam-se à próxima reprodução.
      </p>
    </fieldset>
  );
}
