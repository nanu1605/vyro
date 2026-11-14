// Clokrix Pro's signing keys (2.5, docs/pro.md).
//
//   node scripts/licence-keygen.mjs
//     Makes a new key pair and prints it; writes nothing. The private half goes into the GitHub
//     secret LICENCE_SIGNING_KEY (the checkout signs each activation with it) and nowhere else;
//     the public half into the repository variable CLOKRIX_LICENCE_PUBKEY (built into the app,
//     which checks activations with it). Make it once: a new pair turns off every activation
//     signed with the old one.
//
//   LICENCE_SIGNING_KEY=… node scripts/licence-keygen.mjs --public
//     Prints the public half of the private key given (the deploy checks the two match).

import { createPrivateKey, createPublicKey, generateKeyPairSync } from 'node:crypto';

/** The raw public key, base64url, as the app takes it. */
const raw = (pub) => pub.export({ format: 'jwk' }).x;

if (process.argv.includes('--public')) {
  const pkcs8 = process.env.LICENCE_SIGNING_KEY?.trim();
  if (!pkcs8) {
    console.error('Set LICENCE_SIGNING_KEY to the private key (PKCS #8, base64).');
    process.exit(1);
  }
  const priv = createPrivateKey({ key: Buffer.from(pkcs8, 'base64'), format: 'der', type: 'pkcs8' });
  console.log(raw(createPublicKey(priv)));
} else {
  const { privateKey, publicKey } = generateKeyPairSync('ed25519');
  const pkcs8 = privateKey.export({ format: 'der', type: 'pkcs8' }).toString('base64');
  console.log('Secret LICENCE_SIGNING_KEY (private: GitHub secret only, never in a file or chat):');
  console.log(pkcs8);
  console.log();
  console.log('Variable CLOKRIX_LICENCE_PUBKEY (public):');
  console.log(raw(publicKey));
}
