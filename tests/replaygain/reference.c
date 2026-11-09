/* Independent libebur128 1.2.6 runner. Input files are little-endian interleaved f64 PCM. */
#include <ebur128.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
int main(int argc, char **argv) {
    if (argc < 4 || (argc-1)%3) return 2;
    size_t n=(argc-1)/3;
    ebur128_state **states=calloc(n,sizeof(*states));
    double album_peak=0;
    for (size_t i=0;i<n;i++) {
        unsigned rate=strtoul(argv[2+i*3],NULL,10), channels=strtoul(argv[3+i*3],NULL,10);
        states[i]=ebur128_init(channels,rate,EBUR128_MODE_I|EBUR128_MODE_TRUE_PEAK);
        if (!states[i]) return 3;
        FILE *f=fopen(argv[1+i*3],"rb"); if (!f) return 4;
        double buf[65536]; size_t count;
        while ((count=fread(buf,sizeof(double),65536/channels*channels,f))) {
            if (count%channels || ebur128_add_frames_double(states[i],buf,count/channels)) return 5;
        }
        fclose(f);
        double loudness,sample=0,peak=0,value;
        if (ebur128_loudness_global(states[i],&loudness)) return 6;
        for(unsigned c=0;c<channels;c++) { ebur128_sample_peak(states[i],c,&value);sample=fmax(sample,value); }
        /* Capture loudness before draining the true-peak interpolation tail. */
        double zeros[192*32]={0};
        if(channels>32 || ebur128_add_frames_double(states[i],zeros,192)) return 7;
        for(unsigned c=0;c<channels;c++) { ebur128_true_peak(states[i],c,&value);peak=fmax(peak,value); }
        album_peak=fmax(album_peak,peak);
        printf("track\t%zu\t%.12f\t%.12f\t%.12f\n",i,loudness,sample,peak);
    }
    /* Album states must exclude tail padding. Re-read without flushing. */
    for(size_t i=0;i<n;i++) {
        unsigned rate=strtoul(argv[2+i*3],NULL,10), channels=strtoul(argv[3+i*3],NULL,10);
        ebur128_destroy(&states[i]);states[i]=ebur128_init(channels,rate,EBUR128_MODE_I);
        FILE *f=fopen(argv[1+i*3],"rb");double buf[65536];size_t count;
        while((count=fread(buf,sizeof(double),65536/channels*channels,f)))if(ebur128_add_frames_double(states[i],buf,count/channels))return 8;
        fclose(f);
    }
    double loudness; if(ebur128_loudness_global_multiple(states,n,&loudness))return 9;
    printf("album\tall\t%.12f\t%.12f\n",loudness,album_peak);
    for(size_t i=0;i<n;i++)ebur128_destroy(&states[i]);free(states);return 0;
}
