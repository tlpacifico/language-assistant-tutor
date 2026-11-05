import { useId } from "react";

import { ARTICLE_NOTICE_TEXT } from "../../lib/article-contracts";

/**
 * Autorização do envio automático desta conversa. Nunca vem selecionada: o
 * utilizador tem de a dar de forma explícita, vendo o destino e o aviso.
 */
export default function ArticleConsent({
  destination,
  checked,
  onChange,
}: {
  destination: string | null;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  const id = useId();
  return (
    <fieldset className="article__consent">
      <legend>Envio automático</legend>
      <p id={`${id}-notice`}>{ARTICLE_NOTICE_TEXT}</p>
      <p className="practice__hint">
        Destino: <code>{destination ?? "a carregar…"}</code>
      </p>
      <label>
        <input
          type="checkbox"
          checked={checked}
          disabled={destination === null}
          aria-describedby={`${id}-notice`}
          onChange={(event) => onChange(event.target.checked)}
        />{" "}
        Autorizo o envio automático desta conversa para este destino
      </label>
    </fieldset>
  );
}
