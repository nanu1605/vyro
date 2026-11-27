// What the phone's direct sessions share (1.7, 2.6): the STUN servers, waiting for the phone's
// own addresses before it offers, and whether the path found goes through a TURN server.

export const STUN: RTCIceServer[] = [{ urls: 'stun:stun.cloudflare.com:3478' }, { urls: 'stun:stun.l.google.com:19302' }];
/** The longest wait for the phone's own addresses before offering. */
export const GATHER_MS = 3000;

/** Done once the connection has gathered its addresses, or after `ms`. */
export function gathered(p: RTCPeerConnection, ms: number): Promise<void> {
  if (p.iceGatheringState === 'complete') return Promise.resolve();
  return new Promise((done) => {
    const t = setTimeout(done, ms);
    p.addEventListener('icegatheringstatechange', () => {
      if (p.iceGatheringState === 'complete') {
        clearTimeout(t);
        done();
      }
    });
  });
}

/** From a connection's stats: whether the path in use goes through a TURN server, and the round
 * trip it measures (ms), where it says. */
export function pathOf(stats: RTCStatsReport | null): { relayed: boolean; rtt: number | null } {
  let pairId: string | undefined;
  let nominated: { localCandidateId?: string; currentRoundTripTime?: number } | undefined;
  stats?.forEach((r) => {
    if (r.type === 'transport') pairId ??= r.selectedCandidatePairId;
    else if (r.type === 'candidate-pair' && r.nominated && r.state === 'succeeded') nominated ??= r;
  });
  const pair = (pairId ? stats?.get(pairId) : undefined) ?? nominated;
  const relayed = (pair?.localCandidateId ? stats?.get(pair.localCandidateId) : undefined)?.candidateType === 'relay';
  const rtt = typeof pair?.currentRoundTripTime === 'number' ? Math.round(pair.currentRoundTripTime * 1000) : null;
  return { relayed, rtt };
}
