import { MICROPHONE_PATH } from "./icon-paths";
const PATHS = {
  microphone: MICROPHONE_PATH,
  playback:
    "M4 9.5h3l4.5-3.5v12L7 14.5H4V9.5Zm11.5-.5a4.2 4.2 0 0 1 0 6M18 6.5a7.5 7.5 0 0 1 0 11",
  transcription: "M6 3.5h8l4 4v13H6v-17Zm8 0v4h4M9 12h6m-6 3.5h6",
  synthesis: "M3 12h2l2-6 3 12 3-9 2 3h6",
};

export const ENVIRONMENT_HINTS = {
  microphone: "Microfone de entrada predefinido",
  playback: "Saída de áudio do tutor",
  transcription: "Conversão local da fala em texto",
  synthesis: "Leitura das perguntas em inglês",
};

export default function EnvironmentIcon({
  name,
}: {
  name: keyof typeof PATHS;
}) {
  return (
    <span className="tutor-env-icon" aria-hidden="true">
      <svg
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.7"
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        <path d={PATHS[name]} />
      </svg>
    </span>
  );
}
