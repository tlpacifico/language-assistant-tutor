import { useEffect, useState } from "react";
import type { InterviewClient } from "../../lib/interview-client";
import type { AiSettingsView } from "../../lib/ai-contracts";
import { newRequestId } from "../../lib/request-id";
import { toAppError } from "../../lib/app-error";
export default function AiSettings({ client }: { client: InterviewClient }) {
  const [settings, setSettings] = useState<AiSettingsView | null>(null),
    [key, setKey] = useState(""),
    [error, setError] = useState(""),
    [busy, setBusy] = useState(false);
  useEffect(() => {
    let active = true;
    client
      .loadAiSettings()
      .then((s) => {
        if (active) setSettings(s);
      })
      .catch((e) => {
        if (active) setError(toAppError(e).message);
      });
    return () => {
      active = false;
    };
  }, [client]);
  async function run(work: () => Promise<AiSettingsView>) {
    setBusy(true);
    setError("");
    try {
      setSettings(await work());
    } catch (e) {
      setError(toAppError(e).message);
    } finally {
      setBusy(false);
      setKey("");
    }
  }
  if (!settings)
    return (
      <section>
        <h2>Configuração IA</h2>
        <p role="status">{error || "A carregar configuração…"}</p>
      </section>
    );
  return (
    <section className="practice__panel tutor-ai-settings">
      <h2>Configuração IA</h2>
      <p>
        O áudio fica neste computador. Apenas o texto confirmado, a pergunta e
        os critérios seguem para o serviço externo. A retenção depende do
        fornecedor.
      </p>
      <label>
        URL base HTTPS
        <input
          value={settings.baseUrl}
          disabled={busy}
          onChange={(e) =>
            setSettings({ ...settings, baseUrl: e.target.value })
          }
        />
      </label>
      <label>
        Modelo pago
        <input
          value={settings.model}
          disabled={busy}
          onChange={(e) =>
            setSettings({ ...settings, model: e.target.value, tariff: null })
          }
        />
      </label>
      <fieldset disabled={busy}>
        <legend>Tarifário de referência editável (USD por milhão)</legend>
        <p>
          Data/fonte: {settings.tariff?.observedAt ?? "Sem tarifário"} —{" "}
          {settings.tariff?.sourceUrl ?? ""}
        </p>
        {(
          [
            "inputPerMillion",
            "outputPerMillion",
            "cachedInputPerMillion",
          ] as const
        ).map((name, i) => (
          <label key={name}>
            {["Entrada", "Saída", "Leitura de cache"][i]}
            <input
              type="number"
              min="0"
              step="0.001"
              value={settings.tariff?.[name] ?? ""}
              onChange={(e) => {
                const previous = settings.tariff ?? {
                  model: settings.model,
                  currency: "USD",
                  inputPerMillion: 0,
                  outputPerMillion: 0,
                  cachedInputPerMillion: null,
                  observedAt: new Date().toISOString().slice(0, 10),
                  sourceUrl: "https://opencode.ai/v2/docs/console/models/",
                };
                setSettings({
                  ...settings,
                  tariff: {
                    ...previous,
                    [name]:
                      e.target.value === "" ? null : Number(e.target.value),
                  },
                });
              }}
            />
          </label>
        ))}
      </fieldset>
      <button
        disabled={busy}
        onClick={() =>
          void run(() =>
            client.saveAiSettings({
              requestId: newRequestId(),
              expectedVersion: settings.version,
              input: {
                baseUrl: settings.baseUrl,
                model: settings.model,
                tariff: settings.tariff,
              },
            }),
          )
        }
      >
        Guardar configuração
      </button>
      <p>
        Chave:{" "}
        {settings.credentialConfigured
          ? "configurada no armazenamento protegido"
          : "não configurada"}
        . Mudar o destino remove a chave anterior e exige nova autorização.
      </p>
      <label>
        Chave da API
        <input
          type="password"
          autoComplete="off"
          value={key}
          disabled={busy}
          onChange={(e) => setKey(e.target.value)}
        />
      </label>
      <button
        disabled={busy || !key.trim()}
        onClick={() => void run(() => client.saveAiKey(key))}
      >
        Guardar chave protegida
      </button>
      <button
        disabled={busy}
        onClick={() => void run(() => client.removeAiKey())}
      >
        Remover chave
      </button>
      <p>
        Destinatário: <strong>{settings.baseUrl}</strong>
      </p>
      <button
        disabled={busy}
        onClick={() =>
          void run(() =>
            client.acceptAiConsent({
              requestId: newRequestId(),
              expectedSettingsVersion: settings.version,
              destination: settings.baseUrl,
              noticeVersion: "text-external-v1",
            }),
          )
        }
      >
        Autorizar envio de texto para este destino
      </button>
      <p>
        {settings.consent?.destination === settings.baseUrl
          ? "Envio autorizado para o destino guardado."
          : "Envio não autorizado para este destino."}
      </p>
      {error && <p role="alert">{error}</p>}
    </section>
  );
}
