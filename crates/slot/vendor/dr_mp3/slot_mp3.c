#define DR_MP3_IMPLEMENTATION
#include "dr_mp3.h"

drmp3 *slot_mp3_open(const void *path, unsigned *rate, unsigned *channels,
                     drmp3_uint64 *duration) {
    drmp3 *mp3 = malloc(sizeof(*mp3));
    if (!mp3) return NULL;
#ifdef _WIN32
    drmp3_bool32 ok = drmp3_init_file_w(mp3, (const wchar_t *)path, NULL);
#else
    drmp3_bool32 ok = drmp3_init_file(mp3, (const char *)path, NULL);
#endif
    if (!ok) {
        free(mp3);
        return NULL;
    }
    *rate = mp3->sampleRate;
    *channels = mp3->channels;
    /* The MPEG header's duration is available without scanning the entire file. */
    *duration = mp3->totalPCMFrameCount == (drmp3_uint64)-1
        ? 0 : drmp3_get_pcm_frame_count(mp3);
    return mp3;
}

unsigned slot_mp3_read(drmp3 *mp3, drmp3_int16 *samples, unsigned frames) {
    return (unsigned)drmp3_read_pcm_frames_s16(mp3, frames, samples);
}

void slot_mp3_close(drmp3 *mp3) {
    drmp3_uninit(mp3);
    free(mp3);
}
