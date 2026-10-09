// Match the binary's FNV-1a transport checksum. This detects accidental
// truncation/rewrites; the ledger and host identity govern eligibility.
export function completeDelivery(text,receipt) {
  const bytes=new TextEncoder().encode(text);
  if (String(bytes.length)!==receipt[2]) return false;
  let checksum=0xcbf29ce484222325n;
  for (const byte of bytes) checksum=BigInt.asUintN(64,(checksum^BigInt(byte))*0x100000001b3n);
  return checksum.toString(16).padStart(16,'0')===receipt[3];
}
