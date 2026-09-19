/**
 * Importing a picture gives back its URL.
 *
 * The build decides what that URL is: a fingerprinted file on the hosted site,
 * and the picture itself as a data URI in the single-file build, which is how
 * that file manages to carry everything it needs. TypeScript has no idea about
 * either, so it is told here.
 */
declare module "*.webp" {
  const url: string;
  export default url;
}
