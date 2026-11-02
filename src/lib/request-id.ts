/**
 * Identificador de um pedido de mutação (`requestId`).
 *
 * As mutações da fase 2 recebem um UUID gerado no cliente e conservam-no ao
 * repetir o mesmo pedido; um pedido novo recebe outro identificador. O valor
 * nunca é derivado do conteúdo, para que dois pedidos iguais em momentos
 * diferentes continuem a ser intenções distintas.
 */
export function newRequestId(): string {
  const cryptoApi = globalThis.crypto;
  if (cryptoApi && typeof cryptoApi.randomUUID === "function") {
    return cryptoApi.randomUUID();
  }

  // Reserva para ambientes sem `randomUUID`: UUID versão 4 a partir de bytes
  // aleatórios disponíveis, sem dependências novas.
  const bytes = new Uint8Array(16);
  if (cryptoApi && typeof cryptoApi.getRandomValues === "function") {
    cryptoApi.getRandomValues(bytes);
  } else {
    for (let index = 0; index < bytes.length; index += 1) {
      bytes[index] = Math.floor(Math.random() * 256);
    }
  }
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;

  const hex = Array.from(bytes, (byte) =>
    byte.toString(16).padStart(2, "0"),
  ).join("");
  return [
    hex.slice(0, 8),
    hex.slice(8, 12),
    hex.slice(12, 16),
    hex.slice(16, 20),
    hex.slice(20),
  ].join("-");
}
