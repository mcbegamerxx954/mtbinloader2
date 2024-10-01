#include <stdlib.h>
int resourcePackManager();
int main() {
  void (**vptr)() = *(void (***)())resourcePackManager;
  
  bool (*load)(void*, void*, void*) = (bool (*)(void*, void*, void*))*(vptr + 2); 
  return 0;
}
