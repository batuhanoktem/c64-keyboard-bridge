#ifndef KB_TEST_CONIO_H
#define KB_TEST_CONIO_H
#include <stdint.h>
void gotoxy(uint8_t x, uint8_t y);
void cclear(uint8_t count);
void cputs(const char *text);
int cprintf(const char *format, ...);
void cputc(char character);
void clrscr(void);
#endif
